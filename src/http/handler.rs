use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_session::MemoryStore;

use async_trait::async_trait;

use hyper::body::{Bytes, Incoming};
use hyper::server::conn::{http1, http2};
use hyper::service::Service;
use hyper::{Request, Response, StatusCode, Version};

use hyper_util::rt::{TokioIo, TokioExecutor};

use http_body_util::{BodyExt, Empty};
use http_body_util::combinators::BoxBody;

use crate::handler::{Handler, Context};
use crate::http::utils::UriExt;
use crate::io::ProxyStream;

use super::{HttpError, HttpService};

struct HyperService {
    service: Arc<dyn HttpService + Send + Sync>,
    ctx: Context,
    http_ctx: HttpContext
}

#[derive(Clone)]
pub struct HttpContext {
    pub sessions: MemoryStore
}

impl HyperService {

}

impl Service<Request<Incoming>> for HyperService {
    type Response = Response<BoxBody<Bytes, HttpError>>;
    type Error = Box<dyn Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, mut req: Request<Incoming>) -> Self::Future {
        let service = self.service.clone();
        let server_name = self.ctx.server_name.clone();

        req.extensions_mut().insert(self.http_ctx.clone());
        req.extensions_mut().insert(self.ctx.clone());

        Box::pin(async move {
            if req.version() == Version::HTTP_2 {
                if req.uri().authority().map(|e| e.host()) != server_name.as_ref().map(|e| e.as_str()) {
                    return Ok(Response::builder()
                        .status(StatusCode::MISDIRECTED_REQUEST)
                        .body(BoxBody::new(Empty::new().map_err(From::from)))?
                    );
                }
            }

            *req.uri_mut() = req.uri().clone().normalize_path()?;
            match service.call(req).await {
                Err(e) => {
                    eprintln!("Internal server error: {}", e);
                    let mut res: Self::Response = Response::new(BoxBody::new(
                        Empty::new().map_err(From::from),
                    ));
                    *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    Ok(res)
                }
                res => res.map_err(From::from),
            }
        })
    }
}

pub struct HttpHandler {
    http1: Http1Handler,
    http2: Http2Handler
}

pub struct Http1Handler {
    builder: http1::Builder,
    service: Arc<dyn HttpService + Send + Sync>,
    context: HttpContext
}

pub struct Http2Handler {
    builder: http2::Builder<TokioExecutor>,
    service: Arc<dyn HttpService + Send + Sync>,
    context: HttpContext
}

impl HttpHandler {
    pub fn new(service: Arc<dyn HttpService + Send + Sync>) -> Self {
        let context = HttpContext {
            sessions: MemoryStore::new()
        };

        Self {
            http1: Http1Handler::new_with_context(service.clone(), context.clone()),
            http2: Http2Handler::new_with_context(service, context)
        }
    }
}

#[async_trait]
impl Handler for HttpHandler {
    async fn handle(&self, stream: ProxyStream, ctx: Context) -> Result<(), Box<dyn Error>> {
        match ctx.alpn.as_deref() {
            Some("h2") => self.http2.handle(stream, ctx).await,
            _ => self.http1.handle(stream, ctx).await,
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

        Self {
            builder,
            service,
            context: HttpContext {
                sessions: MemoryStore::new()
            }
        }
    }

    pub fn new_with_context(service: Arc<dyn HttpService + Send + Sync>, context: HttpContext) -> Self {
        let mut builder = http1::Builder::new();
        builder.preserve_header_case(true).title_case_headers(true);

        Self {
            builder,
            service,
            context
        }
    }
}

#[async_trait]
impl Handler for Http1Handler {
    async fn handle(&self, stream: ProxyStream, ctx: Context) -> Result<(), Box<dyn Error>> {
        let service = HyperService {
            service: self.service.clone(),
            http_ctx: self.context.clone(),
            ctx
        };
        self.builder
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades()
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

        Self {
            builder,
            service,
            context: HttpContext {
                sessions: MemoryStore::new()
            }
        }
    }

    pub fn new_with_context(service: Arc<dyn HttpService + Send + Sync>, context: HttpContext) -> Self {
        let builder = http2::Builder::new(TokioExecutor::new());

        Self {
            builder,
            service,
            context
        }
    }
}

#[async_trait]
impl Handler for Http2Handler {
    async fn handle(&self, stream: ProxyStream, ctx: Context) -> Result<(), Box<dyn Error>> {
        let service = HyperService {
            service: self.service.clone(),
            http_ctx: self.context.clone(),
            ctx
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
