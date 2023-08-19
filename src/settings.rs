use async_recursion::async_recursion;

use config::{Config, ConfigError, File};

use serde_derive::Deserialize;

use std::error::Error;
use std::sync::Arc;

use crate::handler::{self};
use crate::listener::{self, TcpListener};
use crate::http::{self, HttpHandler, Http1Handler, Http2Handler, HelloService, ProxyService};
use crate::tls::{TlsHandler, LazyTlsHandler};
use crate::tunnel::TunnelHandler;

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub listeners: Vec<Listener>
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Listener {
    Socket(SocketListener)
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct SocketListener {
    pub listen: String,
    pub handler: Handler
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Tls {
    pub certificate: String,
    pub key: String,
    pub handler: Box<Handler>
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Handler {
    Http(Http),
    Http1(Http),
    Http2(Http),
    Tunnel(Tunnel),
    Tls(Tls),
    LazyTls(Tls)
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Tunnel {
    pub target: String
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Http {
    pub service: Service
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Service {
    Hello,
    Proxy(Proxy)
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Proxy {
    pub uri: String
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config.toml"))
            .build()?;

        s.try_deserialize()
    }
}

pub async fn build_listener(listener: &Listener) -> Result<Box<dyn listener::Listener>, Box<dyn Error>> {
    Ok(match listener {
        Listener::Socket(s) => Box::new(TcpListener::new(&s.listen, build_handler(&s.handler).await?).await?)
    })
}

#[async_recursion]
pub async fn build_handler(handler: &Handler) -> Result<Box<dyn handler::Handler + Send + Sync + Unpin>, Box<dyn Error>> {
    let handler: Box<dyn handler::Handler + Send + Sync + Unpin> = match handler {
        Handler::Tunnel(s) => Box::new(TunnelHandler::new(s.target.clone())),
        Handler::Tls(s) => Box::new(TlsHandler::new(s, build_handler(&s.handler).await?)?),
        Handler::LazyTls(s) => Box::new(LazyTlsHandler::new(s, build_handler(&s.handler).await?)?),
        Handler::Http(s) => Box::new(HttpHandler::new(build_service(&s.service).await?)),
        Handler::Http1(s) => Box::new(Http1Handler::new(build_service(&s.service).await?)),
        Handler::Http2(s) => Box::new(Http2Handler::new(build_service(&s.service).await?))
    };
    Ok(handler)
}

pub async fn build_service(service: &Service) -> Result<Arc<dyn http::HttpService + Send + Sync>, Box<dyn Error>> {
    let service: Arc<dyn http::HttpService + Send + Sync> = match service {
        Service::Hello => Arc::new(HelloService {}),
        Service::Proxy(s) => Arc::new(ProxyService::new((&s.uri).try_into()?))
    };
    Ok(service)
}
