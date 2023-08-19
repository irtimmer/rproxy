use async_trait::async_trait;

use std::error::Error;

use crate::io::SendableAsyncStream;

#[async_trait]
pub trait Handler {
    async fn handle(&self, mut stream: SendableAsyncStream) -> Result<(), Box<dyn Error>>;

    fn alpn_protocols(&self) -> Option<Vec<String>> {
        None
    }
}

pub type SendableHandler = Box<dyn Handler + Send + Sync>;
