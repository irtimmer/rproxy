use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use chrono::Local;

use http_body_util::combinators::BoxBody;

use hyper::body::{Body, Bytes, Incoming};
use hyper::{Request, Response};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::Error;
use crate::handler::Context;

use super::HttpService;

pub struct LogLayer {
    service: Arc<dyn HttpService + Send + Sync>,
    out: Mutex<File>
}

impl LogLayer {
    pub async fn new(service: Arc<dyn HttpService + Send + Sync>, path: &PathBuf) -> Result<Self, Error> {
        Ok(Self {
            service,
            out: Mutex::new(OpenOptions::new().create(true).append(true).open(path).await?)
        })
    }
}

#[async_trait]
impl HttpService for LogLayer {
    async fn call(&self, req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, Error>>, Error> {
        let ctx = req.extensions().get::<Context>().unwrap();
        let remote_addr = ctx.addr.map(|e| e.to_string()).unwrap_or("-".to_owned());
        let remote_user = "-";

        let now = Local::now();
        let time_local = now.format("[%d/%b/%Y:%H:%M:%S %z]").to_string();
        let request = req.uri().path_and_query().map(|e| e.to_string()).unwrap_or("/".to_owned());
        let http_referer = req.headers().get("Referer").map(|e| e.to_str().unwrap()).unwrap_or("-").to_owned();
        let http_user_agent = req.headers().get("User-Agent").map(|e| e.to_str().unwrap()).unwrap_or("-").to_owned();
        let resp = self.service.call(req).await;
        match resp {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body_bytes_sent = resp.body().size_hint().lower();
                let log = format!("{} - {} {} \"{}\" {} {} \"{}\" \"{}\"\n", remote_addr, remote_user, time_local, request, status, body_bytes_sent, http_referer, http_user_agent);
                {
                    let mut out = self.out.lock().await;
                    out.write_all(log.as_bytes()).await?;
                    out.flush().await?;
                }
                Ok(resp)
            }
            Err(e) => {
                let log = format!("{} - {} {} \"{}\" {} {} \"{}\" \"{}\"\n", remote_addr, remote_user, time_local, request, 500, 0, http_referer, http_user_agent);
                {
                    let mut out = self.out.lock().await;
                    out.write_all(log.as_bytes()).await?;
                    out.flush().await?;
                }
                Err(e)
            }
        }
    }
}
