use std::pin::Pin;

use tokio::io::{AsyncWrite, AsyncRead};

pub trait AsyncStream: AsyncWrite + AsyncRead {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub type SendableAsyncStream = Pin<Box<dyn AsyncStream + Send + Sync>>;
