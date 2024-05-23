//! Error handling.

use alloc::string::String;
use snafu::Snafu;

use crate::PathBuf;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Snafu, Debug)]
#[must_use]
pub enum Error {
    #[snafu(display("invalid save version (expected {expected:?}, found: {found:?})"))]
    InvalidSaveVersion {
        expected: &'static str,
        found: String,
    },
    #[snafu(display("invalid tetanes header (path: {path:?}. {error}"))]
    InvalidSaveHeader { path: PathBuf, error: String },
    #[snafu(display("invalid configuration {value:?} for {field:?}"))]
    InvalidConfig { field: &'static str, value: String },
    #[snafu(display("{context}: {inner:?}"))]
    Io {
        context: String,
        inner: crate::io::Error,
    },
    #[snafu(display("{inner}"))]
    Unknown { inner: String },
}

impl Error {
    pub fn io(inner: crate::io::Error, context: impl Into<String>) -> Self {
        Self::Io {
            context: context.into(),
            inner,
        }
    }
}
