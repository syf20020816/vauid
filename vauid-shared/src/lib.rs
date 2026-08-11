//! vauid-shared is a shared library for vauid server and client, which contains common data structures and functions.

use crate::error::Error;

// error for vauid server and client
pub mod error;
// configuration for vauid server
pub mod conf;
// signaling protocol between client and server
pub mod proto;


pub type Result<T> = std::result::Result<T, Error>;