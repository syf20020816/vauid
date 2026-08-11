use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Other(String),
    #[error("QUIC ERROR: {0}")]
    Quic(QuicError),
    #[error("IO ERROR: {0}")]
    IO(std::io::Error),
    #[error("serde error: {0}")]
    Serde(serde_json::Error),
    #[error("config error: {0}")]
    Config(String),
}

#[derive(Debug, Error)]
pub enum QuicError {
    #[error("send to addr not found: {addr}, buf: {buf:?})")]
    SendAddrNotFound {
        addr: SocketAddr,
        buf: Option<Vec<u8>>,
    },
    #[error("recv from addr not found: {addr}, buf: {buf:?})")]
    RecvAddrNotFound {
        addr: SocketAddr,
        buf: Option<Vec<u8>>,
    },
    #[error("config error: {0}")]
    Config(String),
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

impl From<QuicError> for Error {
    fn from(value: QuicError) -> Self {
        Self::Quic(value)
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(value: Box<dyn std::error::Error>) -> Self {
        Self::Other(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(value: toml::ser::Error) -> Self {
        Self::Config(value.to_string())
    }
}