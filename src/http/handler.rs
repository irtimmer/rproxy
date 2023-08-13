use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;

use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::Service;
use hyper::{Request, Response};

use hyper_util::rt::TokioIo;

use http_body_util::combinators::BoxBody;

use crate::handler::Handler;
use crate::io::SendableAsyncStream;

use super::HttpService;

struct HyperService {
    service: Arc<dyn HttpService + Send + Sync>,
}

impl Service<Request<Incoming>> for HyperService {
    type Response = Response<BoxBody<Bytes, Self::Error>>;
    type Error = Box<dyn Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let service = self.service.clone();
        Box::pin(async move { service.call(req).await })
    }
}

pub struct HttpHandler {
    builder: http1::Builder,
    service: Arc<dyn HttpService + Send + Sync>,
}

impl HttpHandler {
    pub fn new(service: Arc<dyn HttpService + Send + Sync>) -> Self {
        let mut builder = http1::Builder::new();
        builder.preserve_header_case(true).title_case_headers(true);

        Self { builder, service }
    }
}

#[async_trait]
impl Handler for HttpHandler {
    async fn handle(&self, stream: SendableAsyncStream) -> Result<(), Box<dyn Error>> {
        let service = HyperService {
            service: self.service.clone(),
        };
        self.builder
            .serve_connection(TokioIo::new(stream), service)
            .await?;
        Ok(())
    }
}
