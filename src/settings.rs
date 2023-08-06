use config::{Config, ConfigError, File};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub listen: String,
    pub handler: Handler,
    pub tls: Option<TLS>
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct TLS {
    pub certificate: String,
    pub key: String
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
#[serde(tag = "type")]
pub enum Handler {
    Tunnel(Tunnel)
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Tunnel {
    pub target: String
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config.toml"))
            .build()?;

        s.try_deserialize()
    }
}
