use std::convert::Infallible;
use std::error::Error;
use std::fmt::Display;

use async_trait::async_trait;

use hyper::body::{Bytes, Incoming};
use hyper::header::InvalidHeaderValue;
use hyper::http::uri::{InvalidUri, InvalidUriParts};
use hyper::{Request, Response};

use http_body_util::combinators::BoxBody;

#[derive(Debug)]
pub enum HttpError {
    Http(hyper::http::Error),
    HyperError(hyper::Error),
    IO(std::io::Error),
    String(String),
    Other(Box<dyn Error + Send + Sync>),
}

impl From<&str> for HttpError {
    fn from(error: &str) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<InvalidUri> for HttpError {
    fn from(error: InvalidUri) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<InvalidUriParts> for HttpError {
    fn from(error: InvalidUriParts) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<InvalidHeaderValue> for HttpError {
    fn from(error: InvalidHeaderValue) -> Self {
        HttpError::String(error.to_string())
    }
}

impl From<hyper::Error> for HttpError {
    fn from(error: hyper::Error) -> Self {
        HttpError::HyperError(error)
    }
}

impl From<hyper::http::Error> for HttpError {
    fn from(error: hyper::http::Error) -> Self {
        HttpError::Http(error)
    }
}

impl From<std::io::Error> for HttpError {
    fn from(error: std::io::Error) -> Self {
        HttpError::IO(error)
    }
}

impl From<Infallible> for HttpError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Http(e) => write!(f, "HTTP error: {}", e),
            HttpError::HyperError(e) => write!(f, "Hyper error: {}", e),
            HttpError::IO(e) => write!(f, "IO error: {}", e),
            HttpError::String(e) => write!(f, "String error: {}", e),
            HttpError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for HttpError {}

#[async_trait]
pub trait HttpService {
    async fn call(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, HttpError>>, HttpError>;
}
