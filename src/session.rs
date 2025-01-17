//! Module contains a Session structure.

use crate::{control_code::ControlCode, error::Error, needle::Needle, stream::Stream, Found};
use std::{
    convert::TryInto,
    io,
    ops::{Deref, DerefMut},
    time::{self, Duration},
};

#[cfg(unix)]
use ptyprocess::{PtyProcess, WaitStatus};
#[cfg(unix)]
use std::process::Command;

#[cfg(not(feature = "async"))]
use io::Write;

#[cfg(all(unix, feature = "async"))]
use futures_lite::AsyncWriteExt;

/// Session represents a process and its streams.
/// It controlls process and communication with it.
#[derive(Debug)]
pub struct Session {
    #[cfg(unix)]
    proc: PtyProcess,
    #[cfg(windows)]
    proc: conpty::Process,
    stream: Stream,
    expect_timeout: Option<Duration>,
}

impl Session {
    /// Spawn spawns a command
    #[cfg(unix)]
    pub fn spawn(command: Command) -> Result<Self, Error> {
        let ptyproc = PtyProcess::spawn(command)?;
        let stream = Stream::new(ptyproc.get_raw_handle()?);

        Ok(Self {
            proc: ptyproc,
            stream,
            expect_timeout: Some(Duration::from_millis(10000)),
        })
    }

    /// Spawn spawns a command
    #[cfg(windows)]
    pub fn spawn(attr: conpty::ProcAttr) -> Result<Self, Error> {
        let proc = attr.spawn()?;
        let stream = Stream::new(proc.input()?, proc.output()?);

        Ok(Self {
            proc,
            stream,
            expect_timeout: Some(Duration::from_millis(10000)),
        })
    }

    /// Expect waits until a pattern is matched.
    ///
    /// If the method returns [Ok] it is guaranteed that at least 1 match was found.
    ///
    /// This make assertions in a lazy manner.
    /// Starts from 1st byte then checks 2nd byte and goes further.
    /// It is done intentinally to be presize.
    /// It matters for example when you call this method with `crate::Regex("\\d+")` and output contains 123,
    /// expect will return '1' as a match not '123'.
    ///
    /// ```
    /// # futures_lite::future::block_on(async {
    /// let mut p = expectrl::spawn("echo 123").unwrap();
    /// let m = p.expect(expectrl::Regex("\\d+")).await.unwrap();
    /// assert_eq!(m.first(), b"1");
    /// # })
    /// ```
    ///
    /// This behaviour is different from [Session::check].
    ///
    /// It return an error if timeout is reached.
    /// You can specify a timeout value by [Session::set_expect_timeout] method.
    #[cfg(feature = "async")]
    pub async fn expect<E: Needle>(&mut self, expect: E) -> Result<Found, Error> {
        let mut checking_data_length = 0;
        let mut eof = false;
        let start = time::Instant::now();
        loop {
            let mut available = self.stream.get_available();
            if checking_data_length == available.len() {
                // We read by byte to make things as lazy as possible.
                //
                // It's chose is important in using Regex as a Needle.
                // Imagine we have a `\d+` regex.
                // Using such buffer will match string `2` imidiately eventhough right after might be other digit.
                //
                // The second reason is
                // if we wouldn't read by byte EOF indication could be lost.
                // And next blocking std::io::Read operation could be blocked forever.
                //
                // We could read all data available via `read_available` to reduce IO operations,
                // but in such case we would need to keep a EOF indicator internally in stream,
                // which is OK if EOF happens onces, but I am not sure if this is a case.
                eof = self.stream.read_available_once(&mut [0; 1]).await? == Some(0);
                available = self.stream.get_available();
            }

            // We intentinally not increase the counter
            // and run check one more time even though the data isn't changed.
            // Because it may be important for custom implementations of Needle.
            if checking_data_length < available.len() {
                checking_data_length += 1;
            }

            let data = &available[..checking_data_length];

            let found = expect.check(data, eof)?;
            if !found.is_empty() {
                let end_index = Found::right_most_index(&found);
                let involved_bytes = data[..end_index].to_vec();
                self.stream.consume_from_buffer(end_index);
                return Ok(Found::new(involved_bytes, found));
            }

            if eof {
                return Err(Error::Eof);
            }

            if let Some(timeout) = self.expect_timeout {
                if start.elapsed() > timeout {
                    return Err(Error::ExpectTimeout);
                }
            }
        }
    }

    /// Expect waits until a pattern is matched.
    ///
    /// If the method returns [Ok] it is guaranteed that at least 1 match was found.
    ///
    /// This make assertions in a lazy manner.
    /// Starts from 1st byte then checks 2nd byte and goes further.
    /// It is done intentinally to be presize.
    /// It matters for example when you call this method with `crate::Regex("\\d+")` and output contains 123,
    /// expect will return '1' as a match not '123'.
    ///
    /// ```
    /// let mut p = expectrl::spawn("echo 123").unwrap();
    /// let m = p.expect(expectrl::Regex("\\d+")).unwrap();
    /// assert_eq!(m.first(), b"1");
    /// ```
    ///
    /// This behaviour is different from [Session::check].
    ///
    /// It return an error if timeout is reached.
    /// You can specify a timeout value by [Session::set_expect_timeout] method.
    #[cfg(not(feature = "async"))]
    pub fn expect<E: Needle>(&mut self, expect: E) -> Result<Found, Error> {
        let mut checking_data_length = 0;
        let mut eof = false;
        let start = time::Instant::now();
        loop {
            let mut available = self.stream.get_available();
            if checking_data_length == available.len() {
                // We read by byte to make things as lazy as possible.
                //
                // It's chose is important in using Regex as a Needle.
                // Imagine we have a `\d+` regex.
                // Using such buffer will match string `2` imidiately eventhough right after might be other digit.
                //
                // The second reason is
                // if we wouldn't read by byte EOF indication could be lost.
                // And next blocking std::io::Read operation could be blocked forever.
                //
                // We could read all data available via `read_available` to reduce IO operations,
                // but in such case we would need to keep a EOF indicator internally in stream,
                // which is OK if EOF happens onces, but I am not sure if this is a case.
                eof = self.stream.read_available_once(&mut [0; 1])? == Some(0);
                available = self.stream.get_available();
            }

            // We intentinally not increase the counter
            // and run check one more time even though the data isn't changed.
            // Because it may be important for custom implementations of Needle.
            if checking_data_length < available.len() {
                checking_data_length += 1;
            }

            let data = &available[..checking_data_length];

            let found = expect.check(data, eof)?;
            if !found.is_empty() {
                let end_index = Found::right_most_index(&found);
                let involved_bytes = data[..end_index].to_vec();
                self.stream.consume_from_buffer(end_index);
                return Ok(Found::new(involved_bytes, found));
            }

            if eof {
                return Err(Error::Eof);
            }

            if let Some(timeout) = self.expect_timeout {
                if start.elapsed() > timeout {
                    return Err(Error::ExpectTimeout);
                }
            }
        }
    }

    /// Check checks if a pattern is matched.
    /// Returns empty found structure if nothing found.
    ///
    /// Is a non blocking version of [Session::expect].
    /// But its strategy of matching is different from it.
    /// It makes search agains all bytes available.
    ///
    /// ```
    /// let mut p = expectrl::spawn("echo 123").unwrap();
    /// // wait to guarantee that check will successed (most likely)
    /// std::thread::sleep(std::time::Duration::from_secs(1));
    /// let m = p.check(expectrl::Regex("\\d+")).unwrap();
    /// assert_eq!(m.first(), b"123");
    /// ```
    #[cfg(not(feature = "async"))]
    pub fn check<E: Needle>(&mut self, needle: E) -> Result<Found, Error> {
        let eof = self.stream.read_available()?;
        let buf = self.stream.get_available();

        let found = needle.check(buf, eof)?;
        if !found.is_empty() {
            let end_index = Found::right_most_index(&found);
            let involved_bytes = buf[..end_index].to_vec();
            self.stream.consume_from_buffer(end_index);
            return Ok(Found::new(involved_bytes, found));
        }

        if eof {
            return Err(Error::Eof);
        }

        Ok(Found::new(Vec::new(), Vec::new()))
    }

    /// Check checks if a pattern is matched.
    /// Returns empty found structure if nothing found.
    ///
    /// Is a non blocking version of [Session::expect].
    /// But its strategy of matching is different from it.
    /// It makes search agains all bytes available.
    ///
    /// ```
    /// # futures_lite::future::block_on(async {
    /// let mut p = expectrl::spawn("echo 123").unwrap();
    /// // wait to guarantee that check will successed (most likely)
    /// std::thread::sleep(std::time::Duration::from_secs(1));
    /// let m = p.check(expectrl::Regex("\\d+")).await.unwrap();
    /// assert_eq!(m.first(), b"123");
    /// # });
    /// ```
    #[cfg(feature = "async")]
    pub async fn check<E: Needle>(&mut self, needle: E) -> Result<Found, Error> {
        let eof = self.stream.read_available().await?;
        let buf = self.stream.get_available();

        let found = needle.check(buf, eof)?;
        if !found.is_empty() {
            let end_index = Found::right_most_index(&found);
            let involved_bytes = buf[..end_index].to_vec();
            self.stream.consume_from_buffer(end_index);
            return Ok(Found::new(involved_bytes, found));
        }

        if eof {
            return Err(Error::Eof);
        }

        Ok(Found::new(Vec::new(), Vec::new()))
    }

    /// Is matched checks if a pattern is matched.
    /// It doesn't consumes bytes from stream.
    ///
    /// Its strategy of matching is different from the one in [Session::expect].
    /// It makes search agains all bytes available.
    ///
    /// If you want to get a matched result [Session::check] and [Session::expect] is a better option,
    /// Because it is not guaranteed that [Session::check] or [Session::expect]
    /// with the same parameters:
    ///  * will successed even right after [Session::is_matched] call.
    ///  * will operate on the same bytes
    ///
    /// IMPORTANT:
    ///
    /// If you call this method with Eof pattern be aware that
    /// eof indication MAY be lost on the next interactions.
    /// It depends from a process you spawn.
    /// So it might be better to use [Session::check] or [Session::expect] with Eof.
    ///
    /// ```
    /// let mut p = expectrl::spawn("echo 123").unwrap();
    /// // wait to guarantee that check will successed (most likely)
    /// std::thread::sleep(std::time::Duration::from_secs(1));
    /// let m = p.is_matched(expectrl::Regex("\\d+")).unwrap();
    /// assert_eq!(m, true);
    /// ```
    #[cfg(not(feature = "async"))]
    pub fn is_matched<E: Needle>(&mut self, needle: E) -> Result<bool, Error> {
        let eof = self.stream.read_available()?;
        let buf = self.stream.get_available();

        let found = needle.check(buf, eof)?;
        if !found.is_empty() {
            return Ok(true);
        }

        if eof {
            return Err(Error::Eof);
        }

        Ok(false)
    }

    /// Is matched checks if a pattern is matched.
    /// It doesn't consumes bytes from stream.
    ///
    /// See sync version [Session::is_matched].
    #[cfg(feature = "async")]
    pub async fn is_matched<E: Needle>(&mut self, needle: E) -> Result<bool, Error> {
        let eof = self.stream.read_available().await?;
        let buf = self.stream.get_available();

        let found = needle.check(buf, eof)?;
        if !found.is_empty() {
            return Ok(true);
        }

        if eof {
            return Err(Error::Eof);
        }

        Ok(false)
    }

    /// Set the pty session's expect timeout.
    pub fn set_expect_timeout(&mut self, expect_timeout: Option<Duration>) {
        self.expect_timeout = expect_timeout;
    }
}

#[cfg(not(feature = "async"))]
impl Session {
    /// Send text to child's `STDIN`.
    ///
    /// To write bytes you can use a [std::io::Write] operations instead.
    pub fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.stream.write_all(s.as_ref().as_bytes())
    }

    /// Send a line to child's `STDIN`.
    pub fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        #[cfg(windows)]
        {
            // win32 has writefilegather function which could be used as write_vectored but it asyncronos which may involve some issue?
            // https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefilegather

            const LINE_ENDING: &[u8] = b"\r\n";
            let _ = self.write_all(s.as_ref().as_bytes())?;
            let _ = self.write_all(LINE_ENDING)?;
            self.flush()?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            const LINE_ENDING: &[u8] = b"\n";

            let bufs = &mut [
                std::io::IoSlice::new(s.as_ref().as_bytes()),
                std::io::IoSlice::new(LINE_ENDING),
                std::io::IoSlice::new(&[]), // we need to add a empty one as it may be not written.
            ];

            // As Write trait says it's not guaranteed that write_vectored will write_all data.
            // But we are sure that write_vectored writes everyting or nothing because underthehood it uses a File.
            // But we rely on this fact not explicitely.
            //
            // todo: check amount of written bytes ands write the rest if not everyting was written already.
            let _ = self.write_vectored(bufs)?;
            self.flush()?;

            Ok(())
        }
    }

    /// Send controll character to a child process.
    ///
    /// You must be carefull passing a char or &str as an argument.
    /// If you pass an unexpected controll you'll get a error.
    /// So it may be better to use [ControlCode].
    ///
    /// ```no_run
    /// use expectrl::{Session, ControlCode};
    /// use std::process::Command;
    ///
    /// #[cfg(unix)]
    /// let mut process = Session::spawn(Command::new("cat")).unwrap();
    /// #[cfg(windows)]
    /// let mut process = Session::spawn(expectrl::ProcAttr::cmd("cat".to_string())).unwrap();
    /// process.send_control(ControlCode::EndOfText); // sends CTRL^C
    /// process.send_control('C'); // sends CTRL^C
    /// process.send_control("^C"); // sends CTRL^C
    /// ```
    pub fn send_control(&mut self, code: impl TryInto<ControlCode>) -> io::Result<()> {
        let code = code.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Failed to parse a control character")
        })?;
        self.stream.write_all(&[code.into()])
    }

    /// Send `EOF` indicator to a child process.
    ///
    /// Often `eof` char handled as it would be a CTRL-C.
    #[cfg(unix)]
    pub fn send_eof(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.proc.get_eof_char()])
    }

    /// Send `INTR` indicator to a child process.
    ///
    /// Often `intr` char handled as it would be a CTRL-D.
    #[cfg(unix)]
    pub fn send_intr(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.proc.get_intr_char()])
    }

    /// Interact gives control of the child process to the interactive user (the
    /// human at the keyboard).
    ///
    /// Returns a status of a process ater interactions.
    /// Why it's crusial to return a status is after check of is_alive the actuall
    /// status might be gone.
    ///
    /// Keystrokes are sent to the child process, and
    /// the `stdout` and `stderr` output of the child process is printed.
    ///
    /// When the user types the `escape_character` this method will return control to a running process.
    /// The escape_character will not be transmitted.
    /// The default for escape_character is entered as `Ctrl-]`, the very same as BSD telnet.
    ///
    /// This simply echos the child `stdout` and `stderr` to the real `stdout` and
    /// it echos the real `stdin` to the child `stdin`.
    #[cfg(unix)]
    pub fn interact(&mut self) -> Result<WaitStatus, Error> {
        crate::interact::InteractOptions::terminal()?.interact(self)
    }

    /// Interact gives control of the child process to the interactive user (the
    /// human at the keyboard).
    #[cfg(windows)]
    pub fn interact(&mut self) -> Result<(), Error> {
        crate::interact::InteractOptions::terminal()?.interact(self)
    }
}

#[cfg(all(feature = "async", not(windows)))]
impl Session {
    /// Send text to child's `STDIN`.
    ///
    /// To write bytes you can use a [std::io::Write] operations instead.
    pub async fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.stream.write_all(s.as_ref().as_bytes()).await
    }

    /// Send a line to child's `STDIN`.
    pub async fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        #[cfg(windows)]
        const LINE_ENDING: &[u8] = b"\r\n";
        #[cfg(not(windows))]
        const LINE_ENDING: &[u8] = b"\n";

        let _ = self.write_all(s.as_ref().as_bytes()).await?;
        let _ = self.write_all(LINE_ENDING).await?;
        self.flush().await?;

        Ok(())
    }

    /// Send controll character to a child process.
    ///
    /// You must be carefull passing a char or &str as an argument.
    /// If you pass an unexpected controll you'll get a error.
    /// So it may be better to use [ControlCode].
    ///
    /// ```no_run
    /// use expectrl::{Session, ControlCode};
    /// use std::process::Command;
    ///
    /// # futures_lite::future::block_on(async {
    /// let mut process = Session::spawn(Command::new("cat")).unwrap();
    /// process.send_control(ControlCode::EndOfText).await.unwrap(); // sends CTRL^C
    /// process.send_control('C').await.unwrap(); // sends CTRL^C
    /// process.send_control("^C").await.unwrap(); // sends CTRL^C
    /// # });
    /// ```
    pub async fn send_control(&mut self, code: impl TryInto<ControlCode>) -> io::Result<()> {
        let code = code.try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Failed to parse a control character")
        })?;
        self.stream.write_all(&[code.into()]).await
    }

    /// Send `EOF` indicator to a child process.
    ///
    /// Often `eof` char handled as it would be a CTRL-C.
    pub async fn send_eof(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.proc.get_eof_char()]).await
    }

    /// Send `INTR` indicator to a child process.
    ///
    /// Often `intr` char handled as it would be a CTRL-D.
    pub async fn send_intr(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.proc.get_intr_char()]).await
    }

    /// Interact gives control of the child process to the interactive user (the
    /// human at the keyboard).
    ///
    /// Returns a status of a process ater interactions.
    /// Why it's crusial to return a status is after check of is_alive the actuall
    /// status might be gone.
    ///
    /// Keystrokes are sent to the child process, and
    /// the `stdout` and `stderr` output of the child process is printed.
    ///
    /// When the user types the `escape_character` this method will return control to a running process.
    /// The escape_character will not be transmitted.
    /// The default for escape_character is entered as `Ctrl-]`, the very same as BSD telnet.
    ///
    /// This simply echos the child `stdout` and `stderr` to the real `stdout` and
    /// it echos the real `stdin` to the child `stdin`.
    pub async fn interact(&mut self) -> Result<WaitStatus, Error> {
        crate::interact::InteractOptions::terminal()?
            .interact(self)
            .await
    }

    pub(crate) async fn read_available_once(
        &mut self,
        buf: &mut [u8],
    ) -> Result<Option<usize>, Error> {
        self.stream
            .read_available_once(buf)
            .await
            .map_err(|err| err.into())
    }

    pub(crate) fn get_available(&mut self) -> &[u8] {
        self.stream.get_available()
    }

    pub(crate) fn consume_from_buffer(&mut self, n: usize) {
        self.stream.consume_from_buffer(n);
    }
}

#[cfg(unix)]
impl Deref for Session {
    type Target = PtyProcess;

    fn deref(&self) -> &Self::Target {
        &self.proc
    }
}

#[cfg(unix)]
impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.proc
    }
}

#[cfg(windows)]
impl Deref for Session {
    type Target = conpty::Process;

    fn deref(&self) -> &Self::Target {
        &self.proc
    }
}

#[cfg(windows)]
impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.proc
    }
}

#[cfg(feature = "async")]
impl Session {
    /// Try to read in a non-blocking mode.
    ///
    /// Returns `[std::io::ErrorKind::WouldBlock]`
    /// in case if there's nothing to read.
    pub async fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.try_read(buf).await
    }

    /// Verifyes if stream is empty or not.
    pub async fn is_empty(&mut self) -> io::Result<bool> {
        self.stream.is_empty().await
    }
}

#[cfg(not(feature = "async"))]
impl Session {
    /// Try to read in a non-blocking mode.
    ///
    /// Returns `[std::io::ErrorKind::WouldBlock]`
    /// in case if there's nothing to read.
    pub fn try_read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.try_read(buf)
    }

    /// Verifyes if stream is empty or not.
    pub fn is_empty(&mut self) -> io::Result<bool> {
        self.stream.is_empty()
    }
}

#[cfg(not(feature = "async"))]
impl std::io::Write for Session {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.stream.write_vectored(bufs)
    }
}

#[cfg(not(feature = "async"))]
impl std::io::Read for Session {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stream.read(buf)
    }
}

#[cfg(not(feature = "async"))]
impl std::io::BufRead for Session {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.stream.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.stream.consume(amt)
    }
}

#[cfg(feature = "async")]
impl futures_lite::io::AsyncWrite for Session {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_close(cx)
    }
}

#[cfg(feature = "async")]
impl futures_lite::io::AsyncRead for Session {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        futures_lite::io::AsyncRead::poll_read(std::pin::Pin::new(&mut self.stream), cx, buf)
    }
}

#[cfg(feature = "async")]
impl futures_lite::io::AsyncBufRead for Session {
    fn poll_fill_buf(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<&[u8]>> {
        let this = self.get_mut();
        let proc = std::pin::Pin::new(&mut this.stream);
        proc.poll_fill_buf(cx)
    }

    fn consume(mut self: std::pin::Pin<&mut Self>, amt: usize) {
        std::pin::Pin::new(&mut self.stream).consume(amt);
    }
}
