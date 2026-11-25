//! A fake identity provider, standing in for `WorkOS` `AuthKit`, Keycloak...: it
//! publishes its metadata and signing keys over HTTP, like a real one, and signs
//! the tokens tests send to the server.
//!
//! It runs inside the test process; the server (a separate process) fetches its
//! keys over HTTP, exactly as in production.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::Jwk, jwk::JwkSet};
use serde_json::{Value, json};

/// A throwaway RSA key used only by these tests (never in production).
const SIGNING_KEY: &[u8] = include_bytes!("../fixtures/test_signing_key.pem");
/// The key id (`kid`) the provider publishes its key under.
pub const KEY_ID: &str = "test-key";
/// A shared secret the provider also publishes (as `kid` `shared-secret`), which
/// the server must never accept: anyone can read it from the key set.
pub const PUBLISHED_SECRET: &[u8] = b"published-secret";

pub struct FakeProvider {
    addr: SocketAddr,
    key: EncodingKey,
    /// How many times the key set was downloaded.
    jwks_fetches: Arc<AtomicUsize>,
}

impl FakeProvider {
    /// Starts serving on a free port, in the background of the test's runtime.
    pub async fn start() -> Self {
        let key = EncodingKey::from_rsa_pem(SIGNING_KEY).unwrap();
        let mut jwk = Jwk::from_encoding_key(&key, Algorithm::RS256).unwrap();
        jwk.common.key_id = Some(KEY_ID.to_string());
        let secret = EncodingKey::from_secret(PUBLISHED_SECRET).unwrap();
        let mut secret_jwk = Jwk::from_encoding_key(&secret, Algorithm::HS256).unwrap();
        secret_jwk.common.key_id = Some("shared-secret".to_string());
        let jwks = JwkSet {
            keys: vec![jwk, secret_jwk],
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let issuer = format!("http://{addr}");
        let jwks_fetches = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&jwks_fetches);
        // Only OpenID Connect's metadata location, like Keycloak: the server
        // tries OAuth's location first, so this also exercises the fallback.
        let app = axum::Router::new()
            .route(
                "/.well-known/openid-configuration",
                axum::routing::get(move || async move {
                    axum::Json(json!({ "issuer": issuer, "jwks_uri": format!("{issuer}/jwks") }))
                }),
            )
            .route(
                "/jwks",
                axum::routing::get(move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    axum::Json(jwks)
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            addr,
            key,
            jwks_fetches,
        }
    }

    pub fn issuer(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn jwks_uri(&self) -> String {
        format!("{}/jwks", self.issuer())
    }

    pub fn jwks_fetches(&self) -> usize {
        self.jwks_fetches.load(Ordering::SeqCst)
    }

    /// The claims of a valid token for user `user-123`, issued for `resource`.
    pub fn claims(&self, resource: &str) -> Value {
        json!({
            "iss": self.issuer(),
            "aud": resource,
            "sub": "user-123",
            "exp": jsonwebtoken::get_current_timestamp() + 300,
        })
    }

    /// A token with these claims, signed with the provider's key.
    pub fn sign(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KEY_ID.to_string());
        self.sign_with_header(&header, claims)
    }

    pub fn sign_with_header(&self, header: &Header, claims: &Value) -> String {
        jsonwebtoken::encode(header, claims, &self.key).unwrap()
    }
}
