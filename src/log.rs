#![cfg(any(feature = "log", feature = "async_log"))]
//! A wrapper of Session to log a read/write operations

use crate::{error::Error, session::Session};
use std::{
    io::{self, Write},
    ops::{Deref, DerefMut},
    process::Command,
};

/// A logging wrapper of session
pub struct SessionWithLog {
    inner: Session,
    logger: Option<Box<dyn Write>>,
}

impl SessionWithLog {
    /// Spawn a session wrapped with logger.
    ///
    /// See [Session].
    pub fn spawn(cmd: &str) -> Result<Self, Error> {
        let session = Session::spawn(cmd)?;
        Ok(Self {
            inner: session,
            logger: None,
        })
    }

    /// Spawn session wrapped with logger.
    ///
    /// See [Session].
    pub fn spawn_cmd(cmd: Command) -> Result<Self, Error> {
        let session = Session::spawn_cmd(cmd)?;
        Ok(Self {
            inner: session,
            logger: None,
        })
    }

    /// Set a writer for which is used for logging.
    ///
    /// Logger is suppose to be called on all IO operations.
    pub fn set_log<W: Write + 'static>(&mut self, w: W) {
        self.logger = Some(Box::new(w));
    }

    fn log(&mut self, target: &str, data: &[u8]) {
        if let Some(writer) = self.logger.as_mut() {
            let _ = match std::str::from_utf8(data) {
                Ok(s) => writeln!(writer, "{} {:?}", target, s),
                Err(..) => writeln!(writer, "{} (bytes) {:?}", target, data),
            };
        }
    }
}

#[cfg(feature = "log")]
impl SessionWithLog {
    pub fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.log("send", s.as_ref().as_bytes());
        self.inner.send(s)
    }

    pub fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.log("send_line", s.as_ref().as_bytes());
        self.inner.send_line(s)
    }
}

#[cfg(feature = "async_log")]
impl SessionWithLog {
    pub async fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.log("send", s.as_ref().as_bytes());
        self.inner.send(s).await
    }

    pub async fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.log("send_line", s.as_ref().as_bytes());
        self.inner.send_line(s).await
    }
}

impl Deref for SessionWithLog {
    type Target = Session;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SessionWithLog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[cfg(feature = "log")]
impl std::io::Write for SessionWithLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.log("write", buf);
        self.deref_mut().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.deref_mut().flush()
    }
}

#[cfg(feature = "log")]
impl std::io::Read for SessionWithLog {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let result = self.deref_mut().read(buf);
        if let Ok(n) = result {
            self.log("read", &buf[..n]);
        }

        result
    }
}

#[cfg(feature = "log")]
impl std::io::BufRead for SessionWithLog {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt)
    }

    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> io::Result<usize> {
        let size = self.inner.read_until(byte, buf)?;
        self.log("read", &buf[..size]);
        Ok(size)
    }

    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        let start_index = buf.as_bytes().len();
        let size = self.inner.read_line(buf)?;
        self.log("read", &buf.as_bytes()[start_index..start_index + size]);
        Ok(size)
    }
}

#[cfg(feature = "async_log")]
impl futures_lite::io::AsyncWrite for SessionWithLog {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.log("write", &buf);
        std::pin::Pin::new(self.inner.deref_mut().deref_mut()).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(self.inner.deref_mut().deref_mut()).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(self.inner.deref_mut().deref_mut()).poll_flush(cx)
    }
}

#[cfg(feature = "async_log")]
impl futures_lite::io::AsyncRead for SessionWithLog {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = futures_lite::io::AsyncRead::poll_read(
            std::pin::Pin::new(self.inner.deref_mut().deref_mut()), // haven't foudn any better way
            cx,
            buf,
        );

        if let std::task::Poll::Ready(Ok(n)) = result {
            self.log("read", &buf[..n]);
        }

        result
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::{
        io::Cursor,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    #[cfg(feature = "sync")]
    #[cfg(feature = "log")]
    #[test]
    fn log() {
        use std::io::Read;

        let mut session = SessionWithLog::spawn("cat").unwrap();
        let writer = StubWriter::default();
        session.set_log(writer.clone());
        session.send_line("Hello World").unwrap();

        // give some time to cat
        // since sometimes we doesn't keep up to read whole string
        thread::sleep(Duration::from_millis(300));

        let mut buf = vec![0; 1024];
        let _ = session.read(&mut buf).unwrap();

        let bytes = writer.inner.lock().unwrap();
        assert_eq!(
            String::from_utf8_lossy(bytes.get_ref()),
            "send_line \"Hello World\"\nread \"Hello World\\r\\n\"\n"
        )
    }

    #[cfg(feature = "sync")]
    #[cfg(feature = "log")]
    #[test]
    fn log_read_line() {
        use std::io::BufRead;

        let mut session = SessionWithLog::spawn("cat").unwrap();
        let writer = StubWriter::default();
        session.set_log(writer.clone());
        session.send_line("Hello World").unwrap();

        let mut buf = String::new();
        let _ = session.read_line(&mut buf).unwrap();
        assert_eq!(buf, "Hello World\r\n");

        let bytes = writer.inner.lock().unwrap();
        assert_eq!(
            String::from_utf8_lossy(bytes.get_ref()),
            "send_line \"Hello World\"\n\
             read \"Hello World\\r\\n\"\n"
        )
    }

    #[cfg(feature = "async_log")]
    #[cfg(feature = "async")]
    #[test]
    fn log() {
        use futures_lite::AsyncReadExt;

        futures_lite::future::block_on(async {
            let mut session = SessionWithLog::spawn("cat").unwrap();
            let writer = StubWriter::default();
            session.set_log(writer.clone());
            session.send_line("Hello World").await.unwrap();

            // give some time to cat
            // since sometimes we doesn't keep up to read whole string
            thread::sleep(Duration::from_millis(300));

            let mut buf = vec![0; 1024];
            let _ = session.read(&mut buf).await.unwrap();

            let bytes = writer.inner.lock().unwrap();
            assert_eq!(
                String::from_utf8_lossy(bytes.get_ref()),
                "send_line \"Hello World\"\nread \"Hello World\\r\\n\"\n"
            )
        })
    }

    #[cfg(feature = "async_log")]
    #[cfg(feature = "async")]
    #[test]
    fn deref() {
        use futures_lite::AsyncReadExt;

        futures_lite::future::block_on(async {
            let mut session = crate::Session::spawn("cat").unwrap();
            let writer = StubWriter::default();
            session.set_log(writer.clone());
            session.send_line("Hello World").await.unwrap();

            let mut buf = vec![0; 1024];
            let _ = session.read(&mut buf).await.unwrap();

            let bytes = writer.inner.lock().unwrap();
            assert_eq!(
                String::from_utf8_lossy(bytes.get_ref()),
                "send_line \"Hello World\"\n\
             read \"Hello World\\r\\n\"\n"
            )
        })
    }

    #[cfg(feature = "async_log")]
    #[cfg(feature = "async")]
    #[test]
    fn log_bash() {
        use futures_lite::AsyncBufReadExt;

        futures_lite::future::block_on(async {
            let mut bash = crate::repl::spawn_bash().await.unwrap();
            let writer = StubWriter::default();
            bash.set_log(writer.clone());
            bash.send_line("echo Hello World").await.unwrap();

            let mut buf = String::new();
            let _ = bash.read_line(&mut buf).await.unwrap();

            let bytes = writer.inner.lock().unwrap();
            assert_eq!(
                String::from_utf8_lossy(bytes.get_ref()),
                "send_line \"echo Hello World\"\n"
            )
        })
    }

    #[derive(Debug, Clone, Default)]
    struct StubWriter {
        inner: Arc<Mutex<Cursor<Vec<u8>>>>,
    }

    impl Write for StubWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.lock().unwrap().flush()
        }
    }
}