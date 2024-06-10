use async_recursion::async_recursion;

use config::{Config, ConfigError, File};

use futures::future::{join_all, try_join_all};
use serde_derive::Deserialize;

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::Error;
use crate::handler::{self};
use crate::listener::{self, TcpListener};
use crate::http::{self, AuthenticatorService, FileService, HelloService, Http1Handler, Http2Handler, HttpHandler, LogLayer, ProxyService, RouterService};
use crate::tls::{self, TlsHandler, LazyTlsHandler};
use crate::tunnel::TunnelHandler;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub servers: Vec<Listener>
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Listener {
    Socket(SocketListener)
}

#[derive(Debug, Deserialize)]
pub struct SocketListener {
    pub listen: String,
    pub handler: Handler
}

#[derive(Debug, Deserialize)]
pub struct SniHandler {
    pub hostname: String,
    pub certificate: String,
    pub key: String,
    pub handler: Box<Handler>
}

#[derive(Debug, Deserialize)]
pub struct Tls {
    pub certificate: String,
    pub key: String,
    pub handler: Box<Handler>,
    pub sni: Vec<SniHandler>
}

#[derive(Debug, Deserialize)]
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
pub struct Tunnel {
    pub target: String
}

#[derive(Debug, Deserialize)]
pub struct Http {
    pub service: Service,
    pub layers: Option<Vec<Layer>>
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Layer {
    Log(Log),
    Authenticator(Authenticator)
}

#[derive(Debug, Deserialize)]
pub struct Log {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Service {
    Hello,
    Proxy(Proxy),
    File(Files),
    Router(Router)
}

#[derive(Debug, Deserialize)]
pub struct Authenticator {
    pub discovery_url: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Deserialize)]
pub struct Proxy {
    pub uri: String
}

#[derive(Debug, Deserialize)]
pub struct Files {
    pub path: String
}

#[derive(Debug, Deserialize)]
pub struct Router {
    pub routes: Vec<Route>
}

#[derive(Debug, Deserialize)]
pub struct Route {
    pub path: String,
    pub service: Service
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config.yaml"))
            .build()?;

        s.try_deserialize()
    }
}

pub async fn build_listener(listener: &Listener) -> Result<Box<dyn listener::Listener>, Error> {
    Ok(match listener {
        Listener::Socket(s) => Box::new(TcpListener::new(&s.listen, build_handler(&s.handler).await?).await?)
    })
}

#[async_recursion]
pub async fn build_handler(handler: &Handler) -> Result<Box<dyn handler::Handler + Send + Sync + Unpin>, Error> {
    let handler: Box<dyn handler::Handler + Send + Sync + Unpin> = match handler {
        Handler::Tunnel(s) => Box::new(TunnelHandler::new(s.target.clone())),
        Handler::Tls(s) => Box::new(TlsHandler::new(s, build_handler(&s.handler).await?)?),
        Handler::LazyTls(s) => Box::new(LazyTlsHandler::new(s, build_handler(&s.handler).await?, try_join_all(s.sni.iter().map(|x| async {
            Ok::<tls::SniHandler, Error>(tls::SniHandler::new(&x.hostname, build_handler(&x.handler).await?, &x.certificate, &x.key)?)
        })).await?)?),
        Handler::Http(s) => Box::new(HttpHandler::new(build_service(&s.service, s.layers.as_ref()).await?)),
        Handler::Http1(s) => Box::new(Http1Handler::new(build_service(&s.service, s.layers.as_ref()).await?)),
        Handler::Http2(s) => Box::new(Http2Handler::new(build_service(&s.service, s.layers.as_ref()).await?))
    };
    Ok(handler)
}

#[async_recursion]
pub async fn build_service(service: &Service, layers: Option<&Vec<Layer>>) -> Result<Arc<dyn http::HttpService + Send + Sync>, Error> {
    let mut service: Arc<dyn http::HttpService + Send + Sync> = match service {
        Service::Hello => Arc::new(HelloService {}),
        Service::Proxy(s) => Arc::new(ProxyService::new((&s.uri).try_into()?)),
        Service::File(s) => Arc::new(FileService::new(&s.path)),
        Service::Router(s) => Arc::new(RouterService::new(join_all(s.routes.iter().map(|x| async {
            http::Route {
                route: x.path.clone(),
                service: build_service(&x.service, None).await.unwrap()
            }
        })).await))
    };

    if let Some(layers) = layers {
        for layer in layers {
            match layer {
                Layer::Log(s) => service = Arc::new(LogLayer::new(service, &s.path).await?),
                Layer::Authenticator(s) => service = Arc::new(AuthenticatorService::new(
                    service,
                    &s.discovery_url,
                    &s.client_id,
                    &s.client_secret
                ).await?)
            }
        }
    }

    Ok(service)
}
