use async_trait::async_trait;

use hyper::{
    body::{Bytes, Incoming},
    Request, Response,
};

use http_body_util::{combinators::BoxBody, BodyExt, Full};

use crate::http::HttpService;

use super::HttpError;

pub struct HelloService {}

#[async_trait]
impl HttpService for HelloService {
    async fn call(&self, _: Request<Incoming>) -> Result<Response<BoxBody<Bytes, HttpError>>, HttpError> {
        let s = "Hello World!";
        let body = BoxBody::new(
            Full::new(Bytes::from(s)).map_err(From::from),
        );
        Ok(Response::builder().body(body)?)
    }
}
