use std::error::Error;

use async_trait::async_trait;

use hyper::{
    body::{Bytes, Incoming},
    Request, Response,
};

use http_body_util::combinators::BoxBody;

#[async_trait]
pub trait HttpService {
    async fn call(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<BoxBody<Bytes, Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>;
}
