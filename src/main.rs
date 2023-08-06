use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

mod io;
mod listener;
mod settings;
mod tls;

use io::IO;
use settings::Settings;
use tls::{load_certs, load_keys};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::new()?;

    println!("Listening on: {}", settings.listen);
    println!("Proxying to: {}", settings.target);

    let listener = TcpListener::bind(settings.listen).await?;
    let acceptor = settings.tls.map(|tls_settings| {
        let certificates = load_certs(Path::new(&tls_settings.certificate)).unwrap();
        let mut keys = load_keys(Path::new(&tls_settings.key)).unwrap();

        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certificates, keys.remove(0))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
            .unwrap();

        TlsAcceptor::from(Arc::new(config))
    });

    while let Ok((inbound, _)) = listener.accept().await {
        let mut inbound: Box<dyn IO + Unpin + Send> = if let Some(acceptor) = &acceptor {
            Box::new(acceptor.accept(inbound).await?)
        } else {
            Box::new(inbound)
        };

        let target = settings.target.clone();
        tokio::spawn(async move {
            let mut outbound = TcpStream::connect(target).await.unwrap();
            let r = copy_bidirectional(&mut inbound, &mut outbound).await;
            if let Err(e) = r {
                println!("Failed to transfer; error={}", e);
            }
        });
    }

    Ok(())
}
