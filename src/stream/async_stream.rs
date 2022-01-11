use std::{
    io::{self, Read, Write},
    os::unix::prelude::AsRawFd,
    pin::Pin,
    task::{Context, Poll},
};

use async_io::Async;
use futures_lite::{AsyncRead, AsyncWrite};

/// Stream represent a IO stream.
#[derive(Debug)]
pub struct AsyncStream<S> {
    inner: Async<S>,
}

impl<S: AsRawFd> AsyncStream<S> {
    /// The function returns a new Stream from a file.
    pub fn new(stream: S) -> io::Result<Self> {
        let stream = Async::new(stream)?;
        Ok(Self { inner: stream })
    }

    pub fn get_mut(&mut self) -> &mut S {
        self.inner.get_mut()
    }
}

impl<S: Write> AsyncWrite for AsyncStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut()).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut()).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut()).poll_close(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut()).poll_write_vectored(cx, bufs)
    }
}

impl<S: Read> AsyncRead for AsyncStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}