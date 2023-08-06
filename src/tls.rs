use rustls_pemfile::{certs, pkcs8_private_keys};

use tokio_rustls::rustls::{Certificate, PrivateKey};

use std::fs::File;
use std::io::{BufReader, Error, ErrorKind, Result};
use std::path::Path;

pub fn load_certs(path: &Path) -> Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())
}

pub fn load_keys(path: &Path) -> Result<Vec<PrivateKey>> {
    pkcs8_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
}
