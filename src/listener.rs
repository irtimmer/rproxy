use async_trait::async_trait;

use tokio::net;
use tokio::io;

use std::sync::Arc;

use crate::handler::SendableHandler;

#[async_trait]
pub trait Listener {
    async fn handle(&self, handler: SendableHandler);
}

pub struct TcpListener {
    listener: net::TcpListener,
}

impl TcpListener {
    pub async fn new(listen: String) -> io::Result<Self> {
        Ok(Self {
            listener: net::TcpListener::bind(listen).await?
        })
    }
}

#[async_trait]
impl Listener for TcpListener {
    async fn handle(&self, handler: SendableHandler) {
        let handler = Arc::new(handler);
        while let Ok((stream, _)) = self.listener.accept().await {
            let handler = handler.clone();
            tokio::spawn(async move {
                let r = handler.handle(Box::pin(stream)).await;
                if let Err(e) = r {
                    println!("Error while handling {}", e);
                }
            });
        }
    }
}
