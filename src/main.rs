use std::error::Error;
use std::sync::Arc;

mod io;
mod handler;
mod listener;
mod settings;

mod tls;
mod tunnel;
mod http;

use handler::SendableHandler;
use tunnel::TunnelHandler;
use listener::{Listener, TcpListener};
use settings::Settings;
use tls::TlsHandler;
use http::{HttpHandler, HttpService, HelloService, ProxyService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::new()?;

    println!("Listening on: {}", settings.listen);

    let mut handler: SendableHandler = match settings.handler {
        settings::Handler::Tunnel(settings) => {
            println!("Proxying to: {}", settings.target);
            Box::pin(TunnelHandler::new(settings.target))
        },
        settings::Handler::Http(settings) => {
            println!("Providing HTTP service");
            let http_service: Arc<dyn HttpService + Send + Sync> = match settings.service {
                settings::Service::Hello => Arc::new(HelloService {}),
                settings::Service::Proxy(settings) => Arc::new(ProxyService::new(settings.uri.try_into()?))
            };
            Box::pin(HttpHandler::new(http_service))
        }
    };

    if let Some(tls_settings) = settings.tls {
        handler = Box::pin(TlsHandler::new(tls_settings, handler)?)
    }

    let listener = TcpListener::new(settings.listen).await?;
    listener.handle(handler).await;

    Ok(())
}
