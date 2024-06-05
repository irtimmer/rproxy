use std::collections::{HashMap, VecDeque};
use std::mem;
use std::result::Result;
use std::sync::Arc;

use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Request, Uri, Version};

use hyper_util::rt::{TokioExecutor, TokioIo};

use http_body_util::combinators::BoxBody;

use tokio::net::{TcpStream, UnixStream};
use tokio::sync::Mutex;

use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;

use crate::{error::Error, io::AsyncStream};

pub enum Connection {
    Http1(http1::SendRequest<BoxBody<Bytes, Error>>),
    Http2(http2::SendRequest<BoxBody<Bytes, Error>>),
    None,
}

type Pool = Arc<Mutex<HashMap<Uri, VecDeque<Connection>>>>;

pub struct Reservation {
    pub conn: Connection,
    uri: Uri,
    pool: Pool,
}

pub struct Client {
    connector: TlsConnector,
    pool: Pool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let pool = self.pool.clone();
        let uri = self.uri.clone();
        let mut conn = Connection::None;
        mem::swap(&mut conn, &mut self.conn);
        tokio::spawn(async move {
            pool.lock().await.entry(uri).or_default().push_back(conn);
        });
    }
}

impl Reservation {
    pub async fn send_request(
        &mut self,
        req: Request<BoxBody<Bytes, Error>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, hyper::Error> {
        self.conn.send_request(req).await
    }
}

impl Connection {
    pub async fn send_request(
        &mut self,
        req: Request<BoxBody<Bytes, Error>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, hyper::Error> {
        match self {
            Connection::Http1(x) => x.send_request(req).await,
            Connection::Http2(x) => x.send_request(req).await,
            Connection::None => unreachable!(),
        }
    }
}

impl Client {
    pub fn new() -> Self {
        let mut config = rustls_platform_verifier::tls_config();
        config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

        Self {
            connector: TlsConnector::from(Arc::new(config)),
            pool: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_connection(&self, uri: &Uri) -> Result<Reservation, Error> {
        let mut pool = self.pool.lock().await;
        if let Some(pool) = pool.get_mut(uri) {
            let mut conn = None;
            while let Some(c) = pool.pop_front() {
                if match &c {
                    Connection::Http1(x) => x.is_ready() && !x.is_closed(),
                    Connection::Http2(x) => x.is_ready() && !x.is_closed(),
                    Connection::None => unreachable!(),
                } {
                    conn = Some(c);
                    break;
                }
            }

            if let Some(conn) = conn {
                return Ok(Reservation {
                    conn,
                    uri: uri.clone(),
                    pool: self.pool.clone(),
                });
            }
        }

        let conn = self.connect(uri).await?;
        Ok(Reservation {
            conn,
            uri: uri.clone(),
            pool: self.pool.clone(),
        })
    }

    async fn connect(&self, uri: &Uri) -> Result<Connection, Error> {
        let (socket, version): (Box<dyn AsyncStream + Send + Unpin>, Version) =
            match uri.scheme().ok_or("No scheme specified")?.as_str() {
                "unix" => (
                    Box::new(UnixStream::connect(uri.path()).await?),
                    Version::HTTP_11,
                ),
                "http" => {
                    let host = uri.host().ok_or("No host specified")?.to_owned();
                    let port = uri.port_u16().unwrap_or(80);
                    let addr = format!("{}:{}", host, port);
                    (Box::new(TcpStream::connect(addr).await?), Version::HTTP_11)
                }
                "https" => {
                    let host = uri.host().ok_or("No host specified")?.to_owned();
                    let port = uri.port_u16().unwrap_or(443);
                    let addr = format!("{}:{}", host, port);
                    let socket = TcpStream::connect(addr).await?;
                    let server_name = ServerName::try_from(host)
                        .or_else(|_| socket.peer_addr().map(|s| ServerName::IpAddress(s.ip().into())))?;
                    let stream = self.connector.connect(server_name, socket).await?;
                    let (_, connection) = stream.get_ref();
                    let protocol = match connection.alpn_protocol() {
                        Some(b"h2") => Version::HTTP_2,
                        _ => Version::HTTP_11,
                    };
                    (Box::new(stream), protocol)
                }
                _ => return Err("Invalid scheme".into()),
            };

        let stream = TokioIo::new(socket);

        Ok(match version {
            Version::HTTP_11 => {
                let (sender, conn) = http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .handshake(stream)
                    .await?;

                tokio::task::spawn(async move {
                    if let Err(err) = conn.await {
                        println!("Connection failed: {:?}", err);
                    }
                });

                Connection::Http1(sender)
            }
            Version::HTTP_2 => {
                let (sender, conn) = http2::Builder::new(TokioExecutor::new())
                    .handshake(stream)
                    .await?;

                tokio::task::spawn(async move {
                    if let Err(err) = conn.await {
                        println!("Connection failed: {:?}", err);
                    }
                });

                Connection::Http2(sender)
            }
            _ => unreachable!(),
        })
    }
}
