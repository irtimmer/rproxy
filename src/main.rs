use std::error::Error;

mod io;
mod handler;
mod listener;
mod settings;

mod tls;
mod tunnel;

use handler::Handler;
use tunnel::TunnelHandler;
use listener::{Listener, TcpListener};
use settings::Settings;
use tls::TlsHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::new()?;

    println!("Listening on: {}", settings.listen);
    println!("Proxying to: {}", settings.target);

    let handler = TunnelHandler::new(settings.target);

    let handler: Box<dyn Handler + Send + Sync + Unpin> = if let Some(tls_settings) = settings.tls {
        Box::new(TlsHandler::new(tls_settings, Box::new(handler))?)
    } else {
        Box::new(handler)
    };

    let listener = TcpListener::new(settings.listen).await?;
    listener.handle(handler).await;

    Ok(())
}
