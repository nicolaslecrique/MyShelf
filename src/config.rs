use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

/// Server configuration, loaded with figment from (lowest to highest priority):
/// 1. built-in defaults,
/// 2. the TOML file `myshelf.toml` (path overridable with `MYSHELF_CONFIG`;
///    a missing file is fine),
/// 3. environment variables prefixed `MYSHELF_` (e.g. `MYSHELF_LISTEN_ADDRESS`).
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// `host:port` the HTTP server binds to.
    pub listen_address: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { listen_address: "127.0.0.1:3000".to_string() }
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
            .merge(Env::prefixed("MYSHELF_"))
            .extract()
            .map_err(Box::new)
    }
}
