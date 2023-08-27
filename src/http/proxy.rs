use std::{error::Error, sync::Arc, mem};

use async_trait::async_trait;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};

use hyper::http::HeaderValue;
use hyper_util::rt::TokioIo;

use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1::Builder;
use hyper::{header, Request, Response, StatusCode, Uri};

use tokio::io::copy_bidirectional;
use tokio::net::{TcpStream, UnixStream};

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
                "unix" => Box::new(UnixStream::connect(self.uri.path()).await?),
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

        let mut proxy_body = BoxBody::new(body.map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }));
        let mut req_body: BoxBody<Bytes, Box<dyn Error + Send + Sync>> = BoxBody::new(Empty::new().map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }));
        if let Some(connection) = req_parts.headers.get(header::CONNECTION) {
            if connection == "upgrade" {
                mem::swap(&mut proxy_body, &mut req_body);
            }
        }

        let mut request = Request::builder().uri(uri).body(proxy_body)?;
        let headers = request.headers_mut();
        headers.clone_from(&req_parts.headers);
        headers.entry(header::HOST).or_insert(HeaderValue::from_str(&host)?);

        let response = sender.send_request(request).await?;
        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            let mut upgrade_response = Response::builder().body(
                Empty::new()
                    .map_err(|e| -> Box<dyn Error + Sync + Send> { Box::new(e) })
                    .boxed(),
            )?;
            *upgrade_response.headers_mut() = response.headers().clone();
            *upgrade_response.status_mut() = response.status();
            let mut upgrade = hyper::upgrade::on(response).await?;
            tokio::spawn(async move {
                match hyper::upgrade::on(Request::from_parts(req_parts, req_body)).await {
                    Ok(mut upstream_upgrade) => {
                        if let Err(e) = copy_bidirectional(&mut TokioIo::new(&mut upgrade), &mut TokioIo::new(&mut upstream_upgrade)).await {
                            eprintln!("Upgrade protocol error: {}", e);
                        };
                    },
                    Err(e) => eprintln!("Upgrade error: {}", e)
                }
            });

            Ok(upgrade_response)
        } else {
            Ok(response.map(|b| {
                b.map_err(|e| -> Box<dyn Error + Sync + Send> { Box::new(e) })
                    .boxed()
            }))
        }
    }
}
