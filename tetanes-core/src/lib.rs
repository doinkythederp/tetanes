#![doc = include_str!("../README.md")]
#![doc(
    html_favicon_url = "https://github.com/lukexor/tetanes/blob/main/assets/tetanes_icon.png?raw=true",
    html_logo_url = "https://github.com/lukexor/tetanes/blob/main/assets/tetanes_icon.png?raw=true"
)]
#![no_std]
#![cfg_attr(target_vendor = "vex", feature(never_type))]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod action;
pub mod apu;
pub mod bus;
pub mod cart;
pub mod fs;
pub mod time;
#[macro_use]
pub mod common;
pub mod control_deck;
pub mod cpu;
pub mod error;
pub mod genie;
pub mod input;
pub mod mapper;
pub mod mem;
pub mod ppu;
pub mod sys;
pub mod video;

#[cfg(not(target_vendor = "vex"))]
pub(crate) use std::io;
#[cfg(not(target_vendor = "vex"))]
pub(crate) use std::path::{Path, PathBuf};
#[cfg(target_vendor = "vex")]
pub(crate) use unix_path::{Path, PathBuf};
#[cfg(target_vendor = "vex")]
pub(crate) use vexide_core::io;

#[cfg(not(target_vendor = "vex"))]
/// File Shim
pub(crate) use std::fs::File;

#[cfg(target_vendor = "vex")]
/// File Shim
pub(crate) struct File;

#[cfg(target_vendor = "vex")]
impl File {
    pub fn open<P: AsRef<Path>>(_path: P) -> io::Result<File> {
        unimplemented!("file open not supported")
    }
}

#[cfg(target_vendor = "vex")]
pub(crate) type BufReader<T> = no_std_io::io::BufReader<T, 1024>;

#[cfg(not(target_vendor = "vex"))]
pub(crate) type BufReader<T> = std::io::BufReader<T>;

pub mod prelude {
    //! The prelude re-exports all the common structs/enums used for basic NES emulation.

    pub use crate::{
        action::Action,
        apu::{Apu, Channel},
        cart::Cart,
        common::{Clock, ClockTo, NesRegion, Regional, Reset, ResetKind, Sample},
        control_deck::{Config, ControlDeck, HeadlessMode},
        cpu::Cpu,
        genie::GenieCode,
        input::{FourPlayer, Input, Player},
        mapper::{Mapped, MappedRead, MappedWrite, Mapper, MapperRevision},
        mem::RamState,
        ppu::{Mirroring, Ppu},
        video::Frame,
    };
}
