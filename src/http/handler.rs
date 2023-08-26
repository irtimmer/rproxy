use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;

use hyper::body::{Bytes, Incoming};
use hyper::server::conn::{http1, http2};
use hyper::service::Service;
use hyper::{Request, Response, Uri};

use hyper_util::rt::{TokioIo, TokioExecutor};

use http_body_util::combinators::BoxBody;

use crate::handler::{Handler, Context};
use crate::io::SendableAsyncStream;

use super::HttpService;

struct HyperService {
    service: Arc<dyn HttpService + Send + Sync>,
}

impl Service<Request<Incoming>> for HyperService {
    type Response = Response<BoxBody<Bytes, Self::Error>>;
    type Error = Box<dyn Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, mut req: Request<Incoming>) -> Self::Future {
        let service = self.service.clone();
        Box::pin(async move {
            normalize_uri(&mut req)?;
            service.call(req).await
        })
    }
}

fn normalize_uri(req: &mut Request<Incoming>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut stack = vec![""];
    let mut trailing_slash = false;
    req.uri().path().split('/').for_each(|e| match e {
        "" | "." => trailing_slash = true,
        ".." => {
            trailing_slash = true;
            if stack.len() > 1 {
                stack.pop();
            }
        },
        _ => {
            trailing_slash = false;
            stack.push(e)
        }
    });
    if trailing_slash {
        stack.push("");
    }
    let path = stack.join("/");
    if path.len() != req.uri().path().len() {
        let mut parts = req.uri().clone().into_parts();
        let path_and_query = match req.uri().query() {
            Some(q) => [&path, q].join("?"),
            None => path
        };
        parts.path_and_query = Some(path_and_query.try_into()?);
        *req.uri_mut() = Uri::from_parts(parts)?;
    };

    Ok(())
}

pub struct HttpHandler {
    http1: Http1Handler,
    http2: Http2Handler
}

pub struct Http1Handler {
    builder: http1::Builder,
    service: Arc<dyn HttpService + Send + Sync>,
}

pub struct Http2Handler {
    builder: http2::Builder<TokioExecutor>,
    service: Arc<dyn HttpService + Send + Sync>,
}

impl HttpHandler {
    pub fn new(service: Arc<dyn HttpService + Send + Sync>) -> Self {
        Self {
            http1: Http1Handler::new(service.clone()),
            http2: Http2Handler::new(service)
        }
    }
}

#[async_trait]
impl Handler for HttpHandler {
    async fn handle(&self, stream: SendableAsyncStream, ctx: Context) -> Result<(), Box<dyn Error>> {
        match ctx.alpn.as_deref() {
            Some("http/1.1") => self.http1.handle(stream, ctx).await,
            Some("h2") => self.http2.handle(stream, ctx).await,
            _ => return Err("Unknown protocol".into())
        }
    }

    fn alpn_protocols(&self) -> Option<Vec<String>> {
        Some(vec!["h2".to_string(), "http/1.1".to_string()])
    }
}

impl Http1Handler {
    pub fn new(service: Arc<dyn HttpService + Send + Sync>) -> Self {
        let mut builder = http1::Builder::new();
        builder.preserve_header_case(true).title_case_headers(true);

        Self { builder, service }
    }
}

#[async_trait]
impl Handler for Http1Handler {
    async fn handle(&self, stream: SendableAsyncStream, _: Context) -> Result<(), Box<dyn Error>> {
        let service = HyperService {
            service: self.service.clone(),
        };
        self.builder
            .serve_connection(TokioIo::new(stream), service)
            .await?;

        Ok(())
    }

    fn alpn_protocols(&self) -> Option<Vec<String>> {
        Some(vec!["http/1.1".to_string()])
    }
}

impl Http2Handler {
    pub fn new(service: Arc<dyn HttpService + Send + Sync>) -> Self {
        let builder = http2::Builder::new(TokioExecutor::new());

        Self { builder, service }
    }
}

#[async_trait]
impl Handler for Http2Handler {
    async fn handle(&self, stream: SendableAsyncStream, _: Context) -> Result<(), Box<dyn Error>> {
        let service = HyperService {
            service: self.service.clone(),
        };
        self.builder
            .serve_connection(TokioIo::new(stream), service)
            .await?;

        Ok(())
    }

    fn alpn_protocols(&self) -> Option<Vec<String>> {
        Some(vec!["h2".to_string()])
    }
}
