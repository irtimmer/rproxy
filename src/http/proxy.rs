use std::{error::Error, sync::Arc};

use async_trait::async_trait;

use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;

use hyper::http::HeaderValue;
use hyper_util::rt::TokioIo;

use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1::Builder;
use hyper::{Request, Response, Uri, header};

use tokio::net::TcpStream;

use tokio_rustls::rustls::{ClientConfig, OwnedTrustAnchor, RootCertStore, ServerName};
use tokio_rustls::TlsConnector;

use crate::io::AsyncStream;

use super::HttpService;

pub struct ProxyService {
    connector: TlsConnector,
    uri: Uri
}

impl ProxyService {
    pub fn new(uri: Uri) -> Self {
        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));

        let config = Arc::new(
            ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_no_client_auth(),
        );

        ProxyService {
            connector: TlsConnector::from(config),
            uri
        }
    }
}

#[async_trait]
impl HttpService for ProxyService {
    async fn call(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>
    {
        let (req_parts, body) = req.into_parts();

        let mut parts = req_parts.uri.clone().into_parts();
        parts.scheme = None;
        parts.authority = None;
        let uri: Uri = Uri::from_parts(parts)?;

        let host = self.uri.host().ok_or("No host specified")?.to_owned();
        let port = self.uri.port_u16().unwrap_or(80);
        let addr = format!("{}:{}", host, port);

        let socket: Box<dyn AsyncStream + Send + Unpin> =
            match self.uri.scheme().ok_or("No scheme specified")?.as_str() {
                "http" => Box::new(TcpStream::connect(addr).await?),
                "https" => {
                    let socket = TcpStream::connect(addr).await?;
                    let server_name = ServerName::try_from(host.as_str())
                        .or_else(|_| socket.peer_addr().map(|s| ServerName::IpAddress(s.ip())))?;
                    Box::new(self.connector.connect(server_name, socket).await?)
                }
                _ => return Err("Invalid scheme".into()),
            };
        let stream = TokioIo::new(socket);

        let (mut sender, conn) = Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .handshake(stream)
            .await?;

        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                println!("Connection failed: {:?}", err);
            }
        });

        let mut request = Request::builder().uri(uri).body(body)?;
        let headers = request.headers_mut();
        headers.clone_from(&req_parts.headers);
        headers.entry(header::HOST).or_insert(HeaderValue::from_str(&host)?);

        let response = sender.send_request(request).await?;
        Ok(response.map(|b| {
            b.map_err(|e| -> Box<dyn Error + Sync + Send> { Box::new(e) })
                .boxed()
        }))
    }
}
