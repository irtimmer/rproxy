use std::sync::Arc;

use async_trait::async_trait;

use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, Uri};

use http_body_util::combinators::BoxBody;

use crate::http::HttpService;

use super::HttpError;

pub struct Route {
    pub route: String,
    pub service: Arc<dyn HttpService + Send + Sync>,
}

pub struct RouterService {
    prefixes: Vec<String>,
    routes: Vec<Route>,
}

impl RouterService {
    pub fn new(routes: Vec<Route>) -> Self {
        Self {
            prefixes: routes.iter().map(|x| x.route.clone()).collect(),
            routes,
        }
    }
}

#[async_trait]
impl HttpService for RouterService {
    async fn call(&self, mut req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, HttpError>>, HttpError> {
        let (index, prefix) = self
            .prefixes
            .iter()
            .enumerate()
            .find(|(_, prefix)| req.uri().path().starts_with(*prefix))
            .ok_or("No route")?;

        let route = self.routes.get(index).unwrap();
        let mut parts = req.uri().clone().into_parts();
        let path_and_query = parts.path_and_query.ok_or("No path and query")?.as_str()
            [prefix.len() - 1..]
            .try_into()?;

        parts.path_and_query = Some(path_and_query);
        *req.uri_mut() = Uri::from_parts(parts)?;
        route.service.call(req).await
    }
}
