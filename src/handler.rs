use async_trait::async_trait;

use std::error::Error;

use crate::io::IO;

#[async_trait]
pub trait Handler {
    async fn handle(&self, mut stream: Box<IO>) -> Result<(), Box<dyn Error>>;
}
