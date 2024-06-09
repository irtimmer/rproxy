use async_trait::async_trait;

use std::error::Error;
use std::net::IpAddr;

use crate::io::SendableAsyncStream;

#[derive(Default, Clone)]
pub struct Context {
    pub secure: bool,
    pub addr: Option<IpAddr>,
    pub alpn: Option<String>,
    pub server_name: Option<String>
}

#[async_trait]
pub trait Handler {
    async fn handle(&self, mut stream: SendableAsyncStream, ctx: Context) -> Result<(), Box<dyn Error>>;

    fn alpn_protocols(&self) -> Option<Vec<String>> {
        None
    }
}

pub type SendableHandler = Box<dyn Handler + Send + Sync>;
