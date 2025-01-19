use std::os::fd::AsRawFd;
use std::pin::Pin;

use ktls::AsyncReadReady;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub trait AsyncStream: AsyncWrite + AsyncRead {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub type SendableAsyncStream = Pin<Box<dyn AsyncStream + Send + Sync>>;

pub enum ProxyStream {
    Tcp(TcpStream),
    Dynamic(Pin<Box<dyn AsyncStream + Send + Sync>>)
}

impl ProxyStream {
    pub fn new_tcp(stream: TcpStream) -> Self {
        ProxyStream::Tcp(stream)
    }

    pub fn new_dynamic(stream: SendableAsyncStream) -> Self {
        ProxyStream::Dynamic(stream)
    }
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(stream) => Pin::new(stream).poll_read(cx, buf),
            ProxyStream::Dynamic(stream) => Pin::new(stream).poll_read(cx, buf)
        }
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ProxyStream::Tcp(stream) => Pin::new(stream).poll_write(cx, buf),
            ProxyStream::Dynamic(stream) => Pin::new(stream).poll_write(cx, buf)
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(stream) => Pin::new(stream).poll_flush(cx),
            ProxyStream::Dynamic(stream) => Pin::new(stream).poll_flush(cx)
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
            ProxyStream::Dynamic(stream) => Pin::new(stream).poll_shutdown(cx)
        }
    }
}

impl AsyncReadReady for ProxyStream {
    fn poll_read_ready(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        match self {
            ProxyStream::Tcp(stream) => stream.poll_read_ready(cx),
            ProxyStream::Dynamic(_) => std::task::Poll::Ready(Ok(()))
        }
    }
}

impl AsRawFd for ProxyStream {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        match self {
            ProxyStream::Tcp(stream) => stream.as_raw_fd(),
            ProxyStream::Dynamic(_) => -1.as_raw_fd()
        }
    }
}
