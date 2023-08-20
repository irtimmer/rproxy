use async_trait::async_trait;

use rustls_pemfile::{certs, pkcs8_private_keys};

use tokio_rustls::rustls::server::Acceptor;
use tokio_rustls::{TlsAcceptor, LazyConfigAcceptor};
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::rustls::ServerConfig;

use std::fs::File;
use std::error::Error;
use std::io::{self, BufReader, ErrorKind};
use std::path::Path;
use std::sync::Arc;

use crate::handler::{SendableHandler, Handler, Context};
use crate::io::SendableAsyncStream;
use crate::settings;

pub struct SniHandler {
    hostname: String,
    handler: SendableHandler,
    certificates: Vec<Certificate>,
    key: PrivateKey
}

pub struct TlsHandler {
    acceptor: TlsAcceptor,
    handler: SendableHandler
}

pub struct LazyTlsHandler {
    handler: SendableHandler,
    certificates: Vec<Certificate>,
    sni: Vec<SniHandler>,
    key: PrivateKey
}

impl SniHandler {
    pub fn new(
        hostname: &str,
        handler: SendableHandler,
        certificate: &str,
        key: &str,
    ) -> io::Result<Self> {
        Ok(Self {
            hostname: hostname.to_owned(),
            handler,
            certificates: load_certs(Path::new(certificate))?,
            key: load_keys(Path::new(key))?.remove(0),
        })
    }
}

impl TlsHandler {
    pub fn new(settings: &settings::Tls, handler: SendableHandler) -> Result<Self, rustls::Error> {
        let certificates = load_certs(Path::new(&settings.certificate)).unwrap();
        let mut keys = load_keys(Path::new(&settings.key)).unwrap();

        let mut config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certificates, keys.remove(0))?;

        if let Some(protocols) = handler.alpn_protocols() {
            config.alpn_protocols.extend(protocols.iter().map(|x| x.as_str().into()));
        }

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            handler
        })
    }
}

impl LazyTlsHandler {
    pub fn new(
        settings: &settings::Tls,
        handler: SendableHandler,
        sni: Vec<SniHandler>,
    ) -> io::Result<Self> {
        Ok(Self {
            handler,
            sni,
            certificates: load_certs(Path::new(&settings.certificate))?,
            key: load_keys(Path::new(&settings.key))?.remove(0)
        })
    }
}

#[async_trait]
impl Handler for TlsHandler {
    async fn handle(&self, stream: SendableAsyncStream, mut ctx: Context) -> Result<(), Box<dyn Error>> {
        let stream = self.acceptor.accept(stream).await?;
        let (_, conn) = stream.get_ref();
        ctx.alpn = conn.alpn_protocol().clone().map(|s| String::from_utf8(s.to_vec())).transpose()?;

        self.handler.handle(Box::pin(stream), ctx).await?;
        Ok(())
    }
}

#[async_trait]
impl Handler for LazyTlsHandler {
    async fn handle(&self, stream: SendableAsyncStream, mut ctx: Context) -> Result<(), Box<dyn Error>> {
        let acceptor = LazyConfigAcceptor::new(Acceptor::default(), stream).await?;

        let (certificates, key, handler) = if let Some(sni) = self
            .sni
            .iter()
            .find(|s| acceptor.client_hello().server_name() == Some(s.hostname.as_str()))
        {
            (&sni.certificates, &sni.key, &sni.handler)
        } else {
            (&self.certificates, &self.key, &self.handler)
        };

        let mut config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certificates.clone(), key.clone())?;

        if let Some(protocols) = handler.alpn_protocols() {
            config.alpn_protocols.extend(protocols.iter().map(|x| x.as_str().into()));
        }

        let stream = acceptor.into_stream(Arc::new(config)).await?;
        let (_, conn) = stream.get_ref();
        ctx.alpn = conn.alpn_protocol().clone().map(|s| String::from_utf8(s.to_vec())).transpose()?;

        handler.handle(Box::pin(stream), ctx).await?;
        Ok(())
    }
}

fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())
}

fn load_keys(path: &Path) -> io::Result<Vec<PrivateKey>> {
    pkcs8_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
}
