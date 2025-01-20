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
    config: Arc<ServerConfig>
}

pub struct TlsHandler {
    acceptor: TlsAcceptor,
    handler: SendableHandler,
    ktls: bool
}

pub struct LazyTlsHandler {
    handler: SendableHandler,
    ktls: bool,
    sni: Vec<SniHandler>,
    config: Arc<ServerConfig>
}

impl SniHandler {
    pub fn new(
        hostname: &str,
        handler: SendableHandler,
        certificate: &str,
        key: &str,
    ) -> Result<Self, rustls::Error> {
        let config = Arc::new(create_config(Path::new(certificate), Path::new(key), handler.alpn_protocols(), true)?);

        Ok(Self {
            hostname: WildMatch::new(hostname),
            handler,
            config
        })
    }
}

impl TlsHandler {
    pub fn new(settings: &settings::Tls, handler: SendableHandler) -> Result<Self, rustls::Error> {
        let ktls = settings.ktls.unwrap_or(false);
        let config = create_config(Path::new(&settings.certificate), Path::new(&settings.key), handler.alpn_protocols(), ktls)?;

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            ktls,
            handler,
        })
    }
}

impl LazyTlsHandler {
    pub fn new(
        settings: &settings::Tls,
        handler: SendableHandler,
        sni: Vec<SniHandler>,
    ) -> Result<Self, rustls::Error> {
        let ktls = settings.ktls.unwrap_or(false);
        let config = Arc::new(create_config(Path::new(&settings.certificate), Path::new(&settings.key), handler.alpn_protocols(), ktls)?);

        Ok(Self {
            ktls,
            handler,
            sni,
            config
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

        let (config, handler) = if let Some(sni) = self
            .sni
            .iter()
            .find(|s| acceptor.client_hello().server_name().map_or(false, |x| s.hostname.matches(x)))
        {
            (&sni.config, &sni.handler)
        } else {
            (&self.config, &self.handler)
        };

        let stream = acceptor.into_stream(config.clone()).await?;
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

fn create_config(certificates: &Path, key: &Path, alpn: Option<Vec<String>>, ktls: bool) -> Result<ServerConfig, rustls::Error> {
    let certificates = certs(&mut BufReader::new(File::open(certificates).unwrap())).collect::<Result<Vec<_>, _>>().unwrap();
    let key = private_key(&mut BufReader::new(File::open(key).unwrap()))
        .and_then(|x| x.ok_or(io::Error::new(ErrorKind::InvalidData, "No private key"))).unwrap();

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, key)?;

    config.enable_secret_extraction = ktls;

    if let Some(protocols) = alpn {
        config.alpn_protocols.extend(protocols.iter().map(|x| x.as_str().into()));
    }

    Ok(config)
}
