use async_trait::async_trait;

use ktls::{config_ktls_server, CorkStream};

use rustls_pemfile::{certs, private_key};

use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::Acceptor;
use tokio_rustls::{TlsAcceptor, LazyConfigAcceptor};
use tokio_rustls::rustls::{self, ServerConfig};

use wildmatch::WildMatch;

use std::fs::File;
use std::error::Error;
use std::io::{self, BufReader, ErrorKind};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use crate::handler::{SendableHandler, Handler, Context};
use crate::io::{ProxyStream, SendableAsyncStream};
use crate::settings;

pub struct SniHandler {
    hostname: WildMatch,
    handler: SendableHandler,
    certificates: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>
}

pub struct TlsHandler {
    acceptor: TlsAcceptor,
    handler: SendableHandler,
    ktls: bool
}

pub struct LazyTlsHandler {
    handler: SendableHandler,
    ktls: bool,
    certificates: Vec<CertificateDer<'static>>,
    sni: Vec<SniHandler>,
    key: PrivateKeyDer<'static>
}

impl SniHandler {
    pub fn new(
        hostname: &str,
        handler: SendableHandler,
        certificate: &str,
        key: &str,
    ) -> io::Result<Self> {
        Ok(Self {
            hostname: WildMatch::new(hostname),
            handler,
            certificates: load_certs(Path::new(certificate))?,
            key: load_key(Path::new(key))?,
        })
    }
}

impl TlsHandler {
    pub fn new(settings: &settings::Tls, handler: SendableHandler) -> Result<Self, rustls::Error> {
        let certificates = load_certs(Path::new(&settings.certificate)).unwrap();
        let key = load_key(Path::new(&settings.key)).unwrap();

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, key)?;

        config.enable_secret_extraction = true;

        if let Some(protocols) = handler.alpn_protocols() {
            config.alpn_protocols.extend(protocols.iter().map(|x| x.as_str().into()));
        }

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            ktls: settings.ktls.unwrap_or(false),
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
            ktls: settings.ktls.unwrap_or(false),
            handler,
            sni,
            certificates: load_certs(Path::new(&settings.certificate))?,
            key: load_key(Path::new(&settings.key))?
        })
    }
}

#[async_trait]
impl Handler for TlsHandler {
    async fn handle(&self, stream: ProxyStream, mut ctx: Context) -> Result<(), Box<dyn Error>> {
        let stream = self.acceptor.accept(CorkStream::new(stream)).await?;
        let (_, conn) = stream.get_ref();
        ctx.alpn = conn.alpn_protocol().clone().map(|s| String::from_utf8(s.to_vec())).transpose()?;
        ctx.server_name = conn.server_name().map(str::to_string);

        let stream: SendableAsyncStream = match self.ktls {
            true => Box::pin(config_ktls_server(stream).await?),
            false => Box::pin(stream)
        };
        self.handler.handle(ProxyStream::new_dynamic(Box::pin(stream)), ctx).await?;
        Ok(())
    }
}

#[async_trait]
impl Handler for LazyTlsHandler {
    async fn handle(&self, stream: ProxyStream, mut ctx: Context) -> Result<(), Box<dyn Error>> {
        let acceptor = LazyConfigAcceptor::new(Acceptor::default(), CorkStream::new(stream)).await?;

        let (certificates, key, handler) = if let Some(sni) = self
            .sni
            .iter()
            .find(|s| acceptor.client_hello().server_name().map_or(false, |x| s.hostname.matches(x)))
        {
            (&sni.certificates, &sni.key, &sni.handler)
        } else {
            (&self.certificates, &self.key, &self.handler)
        };

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates.clone(), key.clone_key())?;

        config.enable_secret_extraction = true;

        if let Some(protocols) = handler.alpn_protocols() {
            config.alpn_protocols.extend(protocols.iter().map(|x| x.as_str().into()));
        }

        let stream = acceptor.into_stream(Arc::new(config)).await?;
        let (_, conn) = stream.get_ref();
        ctx.secure = true;
        ctx.alpn = conn.alpn_protocol().clone().map(|s| String::from_utf8(s.to_vec())).transpose()?;
        ctx.server_name = conn.server_name().map(str::to_string);

        let stream: SendableAsyncStream = match self.ktls {
            true => Box::pin(config_ktls_server(stream).await?),
            false => Box::pin(stream)
        };
        handler.handle(ProxyStream::new_dynamic(Box::pin(stream)), ctx).await?;
        Ok(())
    }
}

fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    certs(&mut BufReader::new(File::open(path)?)).collect()
}

fn load_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    private_key(&mut BufReader::new(File::open(path)?)).and_then(|x| x.ok_or(io::Error::new(ErrorKind::InvalidData, "No private key")))
}
