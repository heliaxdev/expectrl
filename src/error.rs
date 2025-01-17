use std::error;
use std::fmt;
use std::fmt::Display;
use std::io;

/// An main error type used in [crate].
#[derive(Debug)]
pub enum Error {
    IO(io::Error),
    #[cfg(unix)]
    Nix(ptyprocess::Error),
    #[cfg(windows)]
    Win(conpty::Error),
    CommandParsing,
    RegexParsing,
    ExpectTimeout,
    Eof,
    Other(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::IO(err) => write!(f, "IO error {}", err),
            #[cfg(unix)]
            Error::Nix(err) => write!(f, "Nix error {}", err),
            #[cfg(windows)]
            Error::Win(err) => write!(f, "Win error {}", err),
            Error::CommandParsing => write!(f, "Can't parse a command string, please check it out"),
            Error::RegexParsing => write!(f, "Can't parse a regex expression"),
            Error::ExpectTimeout => write!(f, "Reached a timeout for expect type of command"),
            Error::Other(message) => write!(f, "Error {}", message),
            Error::Eof => write!(f, "EOF was reached; the read may successed later"),
        }
    }
}

impl error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

#[cfg(unix)]
impl From<ptyprocess::Error> for Error {
    fn from(err: ptyprocess::Error) -> Self {
        Self::Nix(err)
    }
}

#[cfg(windows)]
impl From<conpty::Error> for Error {
    fn from(err: conpty::Error) -> Self {
        Self::Win(err)
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}
