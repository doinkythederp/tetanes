//! NES Memory Mappers for Cartridges
//!
//! <http://wiki.nesdev.com/w/index.php/Mapper>

use crate::{
    common::{Clocked, Powered},
    ppu::Mirroring,
};
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

pub use m000_nrom::Nrom;
pub use m001_sxrom::Sxrom;
pub use m002_uxrom::Uxrom;
pub use m003_cnrom::Cnrom;
pub use m004_txrom::Txrom;
pub use m005_exrom::Exrom;
pub use m007_axrom::Axrom;
pub use m009_pxrom::Pxrom;
pub use m066_gxrom::Gxrom;
pub use m071_bf909x::Bf909x;

pub mod m000_nrom;
pub mod m001_sxrom;
pub mod m002_uxrom;
pub mod m003_cnrom;
pub mod m004_txrom;
pub mod m005_exrom;
pub mod m007_axrom;
pub mod m009_pxrom;
pub mod m066_gxrom;
pub mod m071_bf909x;

#[enum_dispatch]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Mapper {
    Empty,
    Nrom,
    Sxrom,
    Uxrom,
    Cnrom,
    Txrom,
    Exrom,
    Axrom,
    Pxrom,
    Gxrom,
    Bf909x,
}

impl Default for Mapper {
    fn default() -> Self {
        Empty.into()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum MirroringType {
    Hardware,
    Software(Mirroring),
}

impl Default for MirroringType {
    fn default() -> Self {
        Self::Hardware
    }
}

impl From<Mirroring> for MirroringType {
    fn from(mirroring: Mirroring) -> Self {
        Self::Software(mirroring)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum MappedRead {
    None,
    Chr(usize),
    PrgRom(usize),
    PrgRam(usize),
    Data(u8),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum MappedWrite {
    None,
    Chr(usize, u8),
    PrgRam(usize, u8),
    PrgRamProtect(bool),
}

#[enum_dispatch(Mapper)]
pub trait MapRead {
    #[inline]
    fn map_read(&mut self, addr: u16) -> MappedRead {
        self.map_peek(addr)
    }
    fn map_peek(&self, addr: u16) -> MappedRead;
}

#[enum_dispatch(Mapper)]
pub trait MapWrite {
    fn map_write(&mut self, addr: u16, val: u8) -> MappedWrite;
}

#[enum_dispatch(Mapper)]
pub trait Mapped {
    #[inline]
    fn mirroring(&self) -> MirroringType {
        MirroringType::Hardware
    }

    #[inline]
    #[must_use]
    fn irq_pending(&self) -> bool {
        false
    }

    #[inline]
    #[must_use]
    fn use_ciram(&self, _addr: u16) -> bool {
        self.mirroring() != Mirroring::FourScreen.into()
    }

    #[inline]
    #[must_use]
    fn nametable_page(&self, _addr: u16) -> u16 {
        0x00
    }

    #[inline]
    fn ppu_addr(&mut self, _addr: u16) {}

    #[inline]
    fn ppu_read(&mut self, _addr: u16) {}

    #[inline]
    fn ppu_write(&mut self, _addr: u16, _val: u8) {}
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Empty;

impl MapRead for Empty {
    fn map_peek(&self, _addr: u16) -> MappedRead {
        MappedRead::None
    }
}

impl MapWrite for Empty {
    fn map_write(&mut self, _addr: u16, _val: u8) -> MappedWrite {
        MappedWrite::None
    }
}

impl Mapped for Empty {}
impl Clocked for Empty {}
impl Powered for Empty {}
