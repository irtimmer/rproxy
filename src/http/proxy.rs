use std::{error::Error, mem};

use async_trait::async_trait;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};

use hyper::http::uri::Authority;
use hyper::http::HeaderValue;
use hyper_util::rt::TokioIo;

use hyper::body::{Bytes, Incoming};
use hyper::{header, Request, Response, StatusCode, Uri, Version};

use tokio::io::copy_bidirectional;

use super::client::{Client, Connection};
use super::HttpService;

pub struct ProxyService {
    client: Client,
    uri: Uri
}

impl ProxyService {
    pub fn new(uri: Uri) -> Self {
        ProxyService {
            client: Client::new(),
            uri,
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

        let mut sender = self.client.get_connection(&self.uri).await?;

        let mut proxy_body =
            BoxBody::new(body.map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }));
        let mut req_body: BoxBody<Bytes, Box<dyn Error + Send + Sync>> =
            BoxBody::new(Empty::new().map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }));
        if let Some(connection) = req_parts.headers.get(header::CONNECTION) {
            if connection == "upgrade" {
                mem::swap(&mut proxy_body, &mut req_body);
            }
        }

        let request = match sender.conn {
            Connection::Http1(_) => Request::builder().uri(Uri::from_parts(parts)?),
            Connection::Http2(_) => {
                parts.authority = self.uri.authority().cloned();
                parts.scheme = self.uri.scheme().cloned();
                Request::builder()
                    .uri(Uri::from_parts(parts)?)
                    .version(Version::HTTP_2)
            }
            _ => unreachable!(),
        };

        let mut request = request.method(req_parts.method.clone()).body(proxy_body)?;
        let headers = request.headers_mut();
        headers.clone_from(&req_parts.headers);
        match sender.conn {
            Connection::Http1(_) => {
                let host = match req_parts.version {
                    Version::HTTP_2 => req_parts.uri.authority().map(Authority::host),
                    _ => req_parts
                        .headers
                        .get(header::HOST)
                        .and_then(|e| e.to_str().ok()),
                };

                if let Some(host) = host {
                    headers
                        .entry(header::HOST)
                        .or_insert(HeaderValue::from_str(host)?);
                }
            }
            _ => (),
        };

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
