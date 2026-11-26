// Login for an HTTP endpoint (an axum router). In OAuth terms the server using
// this module is a *resource server*: it never sees passwords, it only checks
// access tokens.
//
// This module is self-contained: it knows nothing of the app using it. Everything
// it needs (provider, resource URL, cache durations...) is passed in
// `AuthSettings`, and it has no defaults of its own.
//
// BACKGROUND: an identity provider (WorkOS AuthKit, Keycloak, any OAuth / OpenID
// Connect provider) logs the user in and gives the client (e.g. an MCP client
// such as Claude) a signed *access token*, a JWT. The client sends it with every
// request (`Authorization: Bearer <token>`) and we check it: signed with one of
// the provider's keys, issued by that provider (`iss`), meant for this server
// (`aud`), not expired (`exp`).
//
// How a client finds the provider (RFC 9728, used by the MCP authorization spec):
// 1. it calls the protected endpoint without a token and gets a 401 whose
//    `WWW-Authenticate` header points at our *protected resource metadata*,
// 2. that small JSON document names the provider (`authorization_servers`),
// 3. the client registers itself with the provider, has the user log in, gets
//    a token, and calls the endpoint again with it.
// So the provider's setup (registering clients, login page...) never touches
// this code: only its issuer URL is given here.

use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{
    Algorithm, AlgorithmFamily, DecodingKey, Validation,
    jwk::{Jwk, JwkSet},
};
use reqwest::Url;
use rmcp::{
    ErrorData,
    handler::server::common::{AsRequestContext, FromContextPart},
};
use serde::{Deserialize, de::DeserializeOwned};

/// Everything `Auth` needs, chosen by the app using it.
#[derive(Clone, Debug)]
pub struct AuthSettings {
    /// The provider's issuer URL, exactly as in its tokens' `iss` claim (and its
    /// metadata's `issuer` field), e.g. `https://auth.example.com`.
    pub issuer: String,
    /// The public URL of the protected endpoint, e.g. `https://api.example.com/mcp`.
    /// Tokens must name it in their `aud` claim, so that a token issued for
    /// another app by the same provider is refused.
    pub resource: String,
    /// Where the provider publishes its signing keys. `None`: found from the
    /// issuer's metadata (`/.well-known/...`) on first use.
    pub jwks_uri: Option<String>,
    /// Human-readable name of the resource, shown to users by some clients
    /// (`resource_name` in the protected resource metadata).
    pub resource_name: Option<String>,
    /// Keys are fetched again after this long, so that a key the provider
    /// removed (rotated out, or revoked after a leak) stops being accepted.
    pub keys_max_age: Duration,
    /// A token signed with an unknown key makes us fetch the keys again (the
    /// provider may have just rotated them), but not more often than this:
    /// anyone can send such tokens, and each one must not cost a request to
    /// the provider.
    pub keys_min_refresh_interval: Duration,
    /// Timeout of each request to the provider (metadata, keys).
    pub http_timeout: Duration,
}

/// The logged-in user making the request.
///
/// A `sub` is only unique within one provider, so the pair identifies a user.
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub issuer: String,
    pub subject: String,
}

/// Lets a tool take the logged-in user as a plain parameter (`user: AuthUser`).
///
/// rmcp hands each tool call the HTTP request's `Parts`; `require_login` stored
/// the user in their extensions (a per-request map keyed by type). For other
/// axum handlers, take `Extension<AuthUser>` instead.
impl<C: AsRequestContext> FromContextPart<C> for AuthUser {
    fn from_context_part(context: &mut C) -> Result<Self, ErrorData> {
        context
            .as_request_context()
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<AuthUser>())
            .cloned()
            // Only reachable if a login-only tool is served without `require_login`.
            .ok_or_else(|| ErrorData::internal_error("this tool needs a logged-in user", None))
    }
}

/// Why a request was refused.
#[derive(Debug)]
enum AuthError {
    /// The token is missing, malformed, expired, forged... The client should
    /// (re)authenticate: 401.
    Invalid(String),
    /// We could not get the provider's keys, so we can't tell: 503, so that
    /// clients retry instead of logging the user out.
    Unavailable,
}

/// The claims we read from a token. Signature, `iss`, `aud` and `exp` are
/// checked by `jsonwebtoken` itself (see `Validation` in `verify`).
#[derive(Deserialize)]
struct Claims {
    sub: String,
}

/// The fields we need from the provider's metadata document.
#[derive(Deserialize)]
struct ProviderMetadata {
    issuer: String,
    jwks_uri: String,
}

/// The provider's signing keys, fetched lazily (not at startup: a cold start
/// must not wait on, or fail because of, the provider).
#[derive(Default)]
struct KeyCache {
    /// Where to fetch the keys: from the settings, or discovered on first use.
    jwks_uri: Option<String>,
    keys: Option<JwkSet>,
    /// Last fetch attempt, successful or not.
    fetched_at: Option<Instant>,
}

/// Checks access tokens. Built once at startup and shared by all requests.
pub struct Auth {
    issuer: String,
    resource: String,
    resource_name: Option<String>,
    keys_max_age: Duration,
    keys_min_refresh_interval: Duration,
    /// Path of our protected resource metadata, e.g.
    /// `/.well-known/oauth-protected-resource/mcp`.
    metadata_path: String,
    /// Its full URL, sent to clients in 401 responses.
    metadata_url: String,
    http: reqwest::Client,
    /// `tokio::sync::Mutex` (not `std`'s) because it is held across `.await`
    /// while fetching. Requests wait for one fetch instead of each starting one.
    keys: tokio::sync::Mutex<KeyCache>,
}

impl Auth {
    /// # Errors
    /// Returns an error if `issuer` or `resource` is not an absolute URL.
    pub fn new(settings: AuthSettings) -> Result<Self, Box<dyn std::error::Error>> {
        Url::parse(&settings.issuer).map_err(|e| format!("issuer {:?}: {e}", settings.issuer))?;
        let resource = Url::parse(&settings.resource)
            .map_err(|e| format!("resource {:?}: {e}", settings.resource))?;
        // RFC 9728: the metadata of `https://host/mcp` is served at
        // `https://host/.well-known/oauth-protected-resource/mcp`.
        let metadata_path = format!(
            "/.well-known/oauth-protected-resource{}",
            resource.path().trim_end_matches('/')
        );
        let metadata_url = format!("{}{metadata_path}", resource.origin().ascii_serialization());
        Ok(Self {
            issuer: settings.issuer,
            resource: settings.resource,
            resource_name: settings.resource_name,
            keys_max_age: settings.keys_max_age,
            keys_min_refresh_interval: settings.keys_min_refresh_interval,
            metadata_path,
            metadata_url,
            http: reqwest::Client::builder()
                .timeout(settings.http_timeout)
                .build()?,
            keys: tokio::sync::Mutex::new(KeyCache {
                jwks_uri: settings.jwks_uri,
                ..KeyCache::default()
            }),
        })
    }

    /// The public routes clients use to discover how to log in.
    pub fn routes(self: &Arc<Self>) -> Router {
        // Served at the RFC 9728 path and, for clients that only try that one,
        // at the bare `/.well-known/oauth-protected-resource`.
        let mut paths = vec!["/.well-known/oauth-protected-resource".to_string()];
        if self.metadata_path != paths[0] {
            paths.push(self.metadata_path.clone());
        }
        paths
            .into_iter()
            .fold(Router::new(), |router, path| {
                router.route(&path, axum::routing::get(protected_resource_metadata))
            })
            .with_state(Arc::clone(self))
    }

    /// Checks a token and returns who it was issued to.
    async fn verify(&self, token: &str) -> Result<AuthUser, AuthError> {
        let invalid = |reason: &str| AuthError::Invalid(reason.to_string());
        let header = jsonwebtoken::decode_header(token).map_err(|_| invalid("malformed token"))?;
        let kid = header
            .kid
            .ok_or_else(|| invalid("no key id (kid) in the token header"))?;
        let jwk = self.key(&kid).await?;

        let key = DecodingKey::from_jwk(&jwk).map_err(|_| invalid("unsupported signing key"))?;
        // A shared-secret (HMAC) key in a public key set makes no sense, and
        // accepting one would let anyone holding it forge tokens.
        if key.family() == AlgorithmFamily::Hmac {
            return Err(invalid("unsupported signing key"));
        }
        // Accept only the algorithms of the key's own family (e.g. RS256 for an
        // RSA key), narrowed to the key's declared algorithm if it has one.
        let mut validation = Validation::new_for_family(key.family());
        if let Some(alg) = jwk.common.key_algorithm {
            validation.algorithms = vec![
                Algorithm::from_str(&alg.to_string())
                    .map_err(|_| invalid("unsupported signing algorithm"))?,
            ];
        }
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.resource]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;

        let data = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|e| AuthError::Invalid(e.to_string()))?;
        Ok(AuthUser {
            issuer: self.issuer.clone(),
            subject: data.claims.sub,
        })
    }

    /// The provider's public key with id `kid`, fetching the keys if needed.
    async fn key(&self, kid: &str) -> Result<Jwk, AuthError> {
        let mut cache = self.keys.lock().await;
        let refresh = match (&cache.keys, cache.fetched_at) {
            (Some(keys), Some(at)) => {
                at.elapsed() > self.keys_max_age
                    || (keys.find(kid).is_none() && at.elapsed() > self.keys_min_refresh_interval)
            }
            // Nothing fetched yet, or every attempt failed so far.
            _ => true,
        };
        if refresh {
            cache.fetched_at = Some(Instant::now());
            match self.fetch_keys(&mut cache.jwks_uri).await {
                Ok(keys) => cache.keys = Some(keys),
                // If we had keys, keep using them until the provider is back.
                Err(error) => tracing::warn!(error, "could not fetch the provider's signing keys"),
            }
        }
        match &cache.keys {
            None => Err(AuthError::Unavailable),
            Some(keys) => keys
                .find(kid)
                .cloned()
                .ok_or_else(|| AuthError::Invalid(format!("unknown signing key {kid:?}"))),
        }
    }

    async fn fetch_keys(&self, jwks_uri: &mut Option<String>) -> Result<JwkSet, String> {
        // Discovered once, then remembered.
        let uri = match jwks_uri {
            Some(uri) => uri.clone(),
            None => jwks_uri.insert(self.discover_jwks_uri().await?).clone(),
        };
        self.get_json(&uri).await
    }

    /// Finds where the provider publishes its keys, from its metadata.
    ///
    /// Providers publish it at one or both of two places: OAuth's (RFC 8414,
    /// e.g. `WorkOS` `AuthKit`) and `OpenID` Connect's (e.g. Keycloak). For an
    /// issuer with a path, they differ in where `.well-known` goes.
    async fn discover_jwks_uri(&self) -> Result<String, String> {
        let issuer = Url::parse(&self.issuer).map_err(|e| e.to_string())?;
        let origin = issuer.origin().ascii_serialization();
        let path = issuer.path().trim_end_matches('/');
        let mut errors = Vec::new();
        for url in [
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ] {
            match self.get_json::<ProviderMetadata>(&url).await {
                // RFC 8414 requires this check: it stops a document served at
                // the issuer's address from pointing at someone else's keys.
                Ok(metadata) if metadata.issuer == self.issuer => return Ok(metadata.jwks_uri),
                Ok(metadata) => errors.push(format!(
                    "{url}: issuer is {:?}, expected {:?}",
                    metadata.issuer, self.issuer
                )),
                Err(error) => errors.push(error),
            }
        }
        Err(errors.join("; "))
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        // An `async` block lets `?` stop at the first error; the error is then
        // turned into a message naming the URL.
        async {
            self.http
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
        }
        .await
        .map_err(|e: reqwest::Error| format!("{url}: {e}"))
    }

    /// A 401 telling the client where to find how to log in.
    fn challenge(&self, error: Option<&str>) -> Response {
        let error = error.map_or_default(|e| format!(r#"error="{e}", "#));
        let value = format!(r#"Bearer {error}resource_metadata="{}""#, self.metadata_url);
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, value)],
        )
            .into_response()
    }
}

/// Middleware: lets the request through only with a valid access token, and
/// makes the user available to the handlers (see `AuthUser`).
pub async fn require_login(
    State(auth): State<Arc<Auth>>,
    mut request: Request,
    next: Next,
) -> Response {
    // `Authorization: Bearer <token>`; the scheme name is case-insensitive.
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim().to_owned());
    let Some(token) = token else {
        return auth.challenge(None);
    };
    match auth.verify(&token).await {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(AuthError::Invalid(reason)) => {
            tracing::info!(reason, "rejected access token");
            auth.challenge(Some("invalid_token"))
        }
        Err(AuthError::Unavailable) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "cannot check access tokens right now, retry later",
        )
            .into_response(),
    }
}

/// RFC 9728 protected resource metadata: tells clients which provider to log
/// in with to call this server.
async fn protected_resource_metadata(State(auth): State<Arc<Auth>>) -> Json<serde_json::Value> {
    let mut metadata = serde_json::json!({
        "resource": auth.resource,
        "authorization_servers": [auth.issuer],
        "bearer_methods_supported": ["header"],
    });
    if let Some(name) = &auth.resource_name {
        metadata["resource_name"] = name.as_str().into();
    }
    Json(metadata)
}
