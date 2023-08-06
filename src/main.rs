use std::error::Error;

mod io;
mod handler;
mod listener;
mod settings;

mod tls;
mod tunnel;

use handler::SendableHandler;
use tunnel::TunnelHandler;
use listener::{Listener, TcpListener};
use settings::Settings;
use tls::TlsHandler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::new()?;

    println!("Listening on: {}", settings.listen);

    let mut handler: SendableHandler = match settings.handler {
        settings::Handler::Tunnel(settings) => {
            println!("Proxying to: {}", settings.target);
            Box::pin(TunnelHandler::new(settings.target))
        }
    };

    if let Some(tls_settings) = settings.tls {
        handler = Box::pin(TlsHandler::new(tls_settings, handler)?)
    }

    let listener = TcpListener::new(settings.listen).await?;
    listener.handle(handler).await;

    Ok(())
}
