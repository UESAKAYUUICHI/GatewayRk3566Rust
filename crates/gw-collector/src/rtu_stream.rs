use std::{pin::Pin, task::{Context, Poll}};
use tokio::{io::{AsyncRead, AsyncWrite, ReadBuf}, net::TcpStream};
use tokio_serial::{ClearBuffer, SerialPort, SerialStream};

pub(super) enum RtuStream {
    Serial(SerialStream),
    Tcp(TcpStream),
}

impl RtuStream {
    pub fn clear_input(&self) -> std::io::Result<()> {
        match self {
            Self::Serial(stream) => stream.clear(ClearBuffer::Input).map_err(std::io::Error::other),
            Self::Tcp(stream) => {
                let mut buffer = [0; 256];
                loop {
                    match stream.try_read(&mut buffer) {
                        Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
}

impl AsyncRead for RtuStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Serial(stream) => Pin::new(stream).poll_read(cx, buffer),
            Self::Tcp(stream) => Pin::new(stream).poll_read(cx, buffer),
        }
    }
}

impl AsyncWrite for RtuStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Serial(stream) => Pin::new(stream).poll_write(cx, buffer),
            Self::Tcp(stream) => Pin::new(stream).poll_write(cx, buffer),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Serial(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tcp(stream) => Pin::new(stream).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Serial(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}
