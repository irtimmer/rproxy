use std::{error::Error, sync::Arc};

use async_trait::async_trait;

use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;

use hyper_util::rt::TokioIo;

use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1::Builder;
use hyper::{Request, Response, Uri};

use tokio::net::TcpStream;

use super::HttpService;

pub struct ProxyService {
    uri: Uri
}

impl ProxyService {
    pub fn new(uri: Uri) -> Self {
        ProxyService {
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
        let host = self.uri.host().ok_or("No host specified")?.to_owned();
        let port = self.uri.port_u16().unwrap_or(80);
        let addr = format!("{}:{}", host, port);

        let socket = TcpStream::connect(addr).await?;
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

        let mut request = Request::builder().uri(req_parts.uri).body(body)?;
        request.headers_mut().clone_from(&req_parts.headers);

        let response = sender.send_request(request).await?;
        Ok(response.map(|b| {
            b.map_err(|e| -> Box<dyn Error + Sync + Send> { Box::new(e) })
                .boxed()
        }))
    }
}
