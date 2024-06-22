use std::path::PathBuf;

use async_trait::async_trait;

use futures::StreamExt;

use http_body_util::{combinators::BoxBody, StreamBody};

use hyper::body::{Bytes, Frame, Incoming};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE};
use hyper::{Request, Response};

use tokio::fs::File;
use tokio_util::io::ReaderStream;

use super::{HttpError, HttpService};

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
    async fn call(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, HttpError>>, HttpError> {
        // UNSAFE: path is not validated
        let path = self.path.join(&req.uri().path()[1..]);
        let file = File::open(path).await?;
        let meta = file.metadata().await?;
        let stream = ReaderStream::new(file);
        let body = StreamBody::new(
            stream.map(|s| s.map(|b| Frame::data(b)).map_err(From::from)),
        );
        let mut builder = Response::builder().header(CONTENT_LENGTH, meta.len());
        if let Some(mime) = mime_guess::from_path(req.uri().path()).first_raw() {
            builder = builder.header(CONTENT_TYPE, mime);
        }

        Ok(builder.body(BoxBody::new(body))?)
    }
}
