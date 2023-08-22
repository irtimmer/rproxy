use std::path::PathBuf;

use async_trait::async_trait;

use futures::StreamExt;

use http_body_util::{combinators::BoxBody, StreamBody};

use hyper::body::{Bytes, Frame, Incoming};
use hyper::{Request, Response};

use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::error::Error;
use super::HttpService;

pub struct FileService {
    path: PathBuf,
}

impl FileService {
    pub fn new(path: &str) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl HttpService for FileService {
    async fn call(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, Error>>, Error> {
        // UNSAFE: path is not validated
        let path = self.path.join(&req.uri().path()[1..]);
        let file = File::open(path).await?;
        let stream = ReaderStream::new(file);
        let body = StreamBody::new(
            stream.map(|s| s.map(|b| Frame::data(b)).map_err(|e| Box::new(e) as Error)),
        );
        let bbody = BoxBody::new(body);
        Ok(Response::builder().body(bbody)?)
    }
}
