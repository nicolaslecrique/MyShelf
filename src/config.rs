use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

/// Server configuration, loaded with figment from (lowest to highest priority):
/// 1. built-in defaults (the single source of truth, suited to local dev),
/// 2. the optional TOML file `myshelf.toml` for local overrides (git-ignored,
///    see `myshelf.example.toml`; path overridable with `MYSHELF_CONFIG`;
///    a missing file is fine),
/// 3. environment variables prefixed `MYSHELF_` (e.g. `MYSHELF_LISTEN_ADDRESS`),
///    which is how deployments configure the server. `__` separates nested keys:
///    `MYSHELF_AUTH__ISSUER` sets `auth.issuer`.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// `host:port` the HTTP server binds to.
    pub listen_address: String,
    /// Host names accepted in the HTTP `Host` header (DNS-rebinding protection).
    /// Behind a platform like Scaleway this must include the public hostname,
    /// otherwise every request is rejected. Env var form: a JSON list, e.g.
    /// `MYSHELF_ALLOWED_HOSTS='["my-app.functions.fnc.fr-par.scw.cloud"]'`.
    /// An empty list (`'[]'`) accepts any host: use it only behind a gateway
    /// that already routes by hostname (the Scaleway deployment does).
    pub allowed_hosts: Vec<String>,
    /// Which logs to keep: a default level plus optional
    /// per-crate levels, e.g. `info` or `debug,rmcp=warn`.
    pub log_filter: String,
    /// Login for the `/mcp` endpoint. Without it, `/mcp` is not served (only
    /// `/public/mcp` is): nothing is ever exposed unauthenticated by mistake.
    pub auth: Option<AuthConfig>,
}

/// The `OAuth` / `OpenID Connect` provider users log in with (`WorkOS` `AuthKit`,
/// Keycloak, Logto...). `MyShelf` only checks the access tokens it issues, so
/// nothing here is specific to one provider.
#[derive(Debug, Deserialize, Serialize)]
pub struct AuthConfig {
    /// The provider's issuer URL, exactly as in its tokens' `iss` claim (and its
    /// metadata's `issuer` field), e.g. `https://auth.example.com`.
    pub issuer: String,
    /// This server's public `/mcp` URL, e.g. `https://mcp.example.com/mcp`.
    /// Tokens must name it in their `aud` claim, so that a token issued for
    /// another app by the same provider is refused here.
    pub resource: String,
    /// Seconds after which the provider's signing keys are fetched again, so
    /// that a key it removed (rotated out, revoked) stops being accepted.
    #[serde(default = "AuthConfig::default_keys_max_age_secs")]
    pub keys_max_age_secs: u64,
    /// Minimum seconds between two fetches of the keys caused by tokens signed
    /// with an unknown key (anyone can send such tokens).
    #[serde(default = "AuthConfig::default_keys_min_refresh_interval_secs")]
    pub keys_min_refresh_interval_secs: u64,
    /// Timeout, in seconds, of each request to the provider.
    #[serde(default = "AuthConfig::default_http_timeout_secs")]
    pub http_timeout_secs: u64,
}

// WHY serde defaults here rather than in `Config::default`: `auth` is optional
// (`None` by default), so its fields can't have defaults there; these fill in
// whatever an `[auth]` section leaves out.
impl AuthConfig {
    fn default_keys_max_age_secs() -> u64 {
        3600
    }

    fn default_keys_min_refresh_interval_secs() -> u64 {
        30
    }

    fn default_http_timeout_secs() -> u64 {
        5
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_address: "127.0.0.1:3000".to_string(),
            allowed_hosts: ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec(),
            log_filter: "info,rmcp=warn".to_string(),
            auth: None,
        }
    }
}

impl Config {
    /// Loads the configuration from defaults, the TOML file and the environment.
    ///
    /// # Errors
    /// Returns an error if the file is malformed or a value has the wrong type.
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let path = std::env::var("MYSHELF_CONFIG").unwrap_or_else(|_| "myshelf.toml".to_string());
        Figment::from(Serialized::defaults(Self::default()))
            .merge(Toml::file(path))
            .merge(Env::prefixed("MYSHELF_").split("__"))
            .extract()
            .map_err(Box::new)
    }
}
