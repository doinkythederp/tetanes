//! Web-specific filesystem operations.

use core::convert::Infallible;

use crate::fs::{Error, Result};
use crate::{
    io::{Read, Write},
    Path,
};

pub fn writer_impl(_path: impl AsRef<Path>) -> Result<impl Write> {
    // TODO: provide file download
    Err::<&'static mut [u8], _>(Error::custom("not implemented: wasm write"))
}

pub fn reader_impl(_path: impl AsRef<Path>) -> Result<impl Read> {
    // TODO: provide file upload?
    Err::<&'static [u8], _>(Error::custom("not implemented: wasm read"))
}

pub fn clear_dir_impl(_path: impl AsRef<Path>) -> Result<()> {
    // TODO: clear storage
    Err::<(), _>(Error::custom("not implemented: wasm clear dir"))
}
