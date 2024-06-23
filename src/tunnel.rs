use async_trait::async_trait;

use tokio::net::TcpStream;
use tokio::io::copy_bidirectional;

use std::error::Error;

use crate::handler::{Handler, Context};
use crate::io::ProxyStream;

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
    async fn handle(&self, mut inbound: ProxyStream, _: Context) -> Result<(), Box<dyn Error>> {
        let mut outbound = TcpStream::connect(&self.target).await?;
        let r = copy_bidirectional(&mut inbound, &mut outbound).await;
        if let Err(e) = r {
            println!("Failed to transfer; error={}", e);
        }
        Ok(())
    }
}
