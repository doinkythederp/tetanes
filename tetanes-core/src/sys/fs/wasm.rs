//! Web-specific filesystem operations.

use crate::fs::{Error, Result};
use core::{
    io::{Empty, Read, Write},
    path::Path,
};

pub fn writer_impl(_path: impl AsRef<Path>) -> Result<impl Write> {
    // TODO: provide file download
    Err::<Empty, _>(Error::custom("not implemented: wasm write"))
}

pub fn reader_impl(_path: impl AsRef<Path>) -> Result<impl Read> {
    // TODO: provide file upload?
    Err::<Empty, _>(Error::custom("not implemented: wasm read"))
}

pub fn clear_dir_impl(_path: impl AsRef<Path>) -> Result<()> {
    // TODO: clear storage
    Err::<(), _>(Error::custom("not implemented: wasm clear dir"))
}
