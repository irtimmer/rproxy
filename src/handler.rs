use async_trait::async_trait;

use std::{error::Error, pin::Pin};

use crate::io::SendableAsyncStream;

#[async_trait]
pub trait Handler {
    async fn handle(&self, mut stream: SendableAsyncStream) -> Result<(), Box<dyn Error>>;
}

pub type SendableHandler = Pin<Box<dyn Handler + Send + Sync>>;
