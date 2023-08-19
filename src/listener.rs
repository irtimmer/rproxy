use async_trait::async_trait;

use tokio::net;
use tokio::io;

use std::sync::Arc;

use crate::handler::Handler;
use crate::handler::SendableHandler;

#[async_trait]
pub trait Listener {
    async fn handle(&self);
}

pub struct TcpListener {
    listener: net::TcpListener,
    handler: Arc<dyn Handler + Send + Sync>
}

impl TcpListener {
    pub async fn new(listen: &str, handler: SendableHandler) -> io::Result<Self> {
        Ok(Self {
            listener: net::TcpListener::bind(listen).await?,
            handler: handler.into()
        })
    }
}

#[async_trait]
impl Listener for TcpListener {
    async fn handle(&self) {
        while let Ok((stream, _)) = self.listener.accept().await {
            let handler = self.handler.clone();
            tokio::spawn(async move {
                let r = handler.handle(Box::pin(stream)).await;
                if let Err(e) = r {
                    println!("Error while handling {}", e);
                }
            });
        }
    }
}
