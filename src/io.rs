use tokio::{io::{AsyncWrite, AsyncRead}, net::TcpStream};
use tokio_rustls::server::TlsStream;

pub trait IO: AsyncWrite + AsyncRead {}

impl IO for TcpStream {}
impl IO for TlsStream<TcpStream> {}
