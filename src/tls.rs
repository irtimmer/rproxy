use async_trait::async_trait;

use rustls_pemfile::{certs, pkcs8_private_keys};

use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::rustls::ServerConfig;

use std::fs::File;
use std::error::Error;
use std::io::{self, BufReader, ErrorKind};
use std::path::Path;
use std::sync::Arc;

use crate::handler::Handler;
use crate::io::IO;
use crate::settings;

pub struct TlsHandler {
    acceptor: TlsAcceptor,
    handler: Box<dyn Handler + Sync + Send + Unpin>
}

impl TlsHandler {
    pub fn new(settings: settings::TLS, handler: Box<dyn Handler + Sync + Send + Unpin>) -> Result<Self, rustls::Error> {
        let certificates = load_certs(Path::new(&settings.certificate)).unwrap();
        let mut keys = load_keys(Path::new(&settings.key)).unwrap();

        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certificates, keys.remove(0))?;

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            handler
        })
    }
}

#[async_trait]
impl Handler for TlsHandler {
    async fn handle(&self, stream: Box<IO>) -> Result<(), Box<dyn Error>> {
        let stream = self.acceptor.accept(stream).await?;

        self.handler.handle(Box::new(stream)).await?;
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
