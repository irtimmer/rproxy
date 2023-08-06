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

    let mut handler: Box<dyn Handler + Send + Sync + Unpin> = match settings.handler {
        settings::Handler::Tunnel(settings) => {
            println!("Proxying to: {}", settings.target);
            Box::new(TunnelHandler::new(settings.target))
        }
    };

    if let Some(tls_settings) = settings.tls {
        handler = Box::new(TlsHandler::new(tls_settings, handler)?)
    }

    let listener = TcpListener::new(settings.listen).await?;
    listener.handle(handler).await;

    Ok(())
}
