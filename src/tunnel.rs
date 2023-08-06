use async_trait::async_trait;

use tokio::net::TcpStream;
use tokio::io::copy_bidirectional;

use std::error::Error;

use crate::handler::Handler;
use crate::io::IO;

pub struct TunnelHandler {
    target: String
}

impl TunnelHandler {
    pub fn new(target: String) -> Self {
        Self {
            target
        }
    }
}

#[async_trait]
impl Handler for TunnelHandler {
    async fn handle(&self, mut inbound: Box<IO>) -> Result<(), Box<dyn Error>> {
        let mut outbound = TcpStream::connect(&self.target).await?;
        let r = copy_bidirectional(&mut inbound, &mut outbound).await;
        if let Err(e) = r {
            println!("Failed to transfer; error={}", e);
        }
        Ok(())
    }
}
