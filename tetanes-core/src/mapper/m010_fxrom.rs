//! `FxROM`/`MMC4` (Mapper 010)
//!
//! <http://wiki.nesdev.com/w/index.php/MMC4>

use crate::{
    cart::Cart,
    common::{Clock, Regional, Reset, ResetKind},
    mapper::{Mapped, MappedRead, MappedWrite, Mapper, MemMap, Mirroring},
    mem::MemBanks,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Fxrom {
    pub mirroring: Mirroring,
    // CHR-ROM $FD/0000 bank select ($B000-$BFFF)
    // CHR-ROM $FE/0000 bank select ($C000-$CFFF)
    // CHR-ROM $FD/1000 bank select ($D000-$DFFF)
    // CHR-ROM $FE/1000 bank select ($E000-$EFFF)
    // 7  bit  0
    // ---- ----
    // xxxC CCCC
    //    | ||||
    //    +-++++- Select 4K CHR-ROM bank for PPU $0000/$1000-$0FFF/$1FFF
    //            used when latch 0/1 = $FD/$FE
    pub latch: [usize; 2],
    pub latch_banks: [u8; 4],
    pub chr_banks: MemBanks,
    pub prg_rom_banks: MemBanks,
}

impl Fxrom {
    const PRG_WINDOW: usize = 16 * 1024;
    const CHR_ROM_WINDOW: usize = 4 * 1024;
    const PRG_RAM_SIZE: usize = 8 * 1024;

    const MIRRORING_MASK: u8 = 0x01;

    pub fn load(cart: &mut Cart) -> Mapper {
        cart.add_prg_ram(Self::PRG_RAM_SIZE);
        let mut fxrom = Self {
            mirroring: cart.mirroring(),
            latch: [0x00; 2],
            latch_banks: [0x00; 4],
            chr_banks: MemBanks::new(0x0000, 0x1FFF, cart.chr_rom.len(), Self::CHR_ROM_WINDOW),
            prg_rom_banks: MemBanks::new(0x8000, 0xFFFF, cart.prg_rom.len(), Self::PRG_WINDOW),
        };
        let last_bank = fxrom.prg_rom_banks.last();
        fxrom.prg_rom_banks.set(1, last_bank);
        fxrom.into()
    }

    pub fn update_banks(&mut self) {
        let bank0 = self.latch_banks[self.latch[0]] as usize;
        let bank1 = self.latch_banks[self.latch[1] + 2] as usize;
        self.chr_banks.set(0, bank0);
        self.chr_banks.set(1, bank1);
    }
}

impl Mapped for Fxrom {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
    }
}

impl MemMap for Fxrom {
    // PPU $0000..=$0FFF Two 4K switchable CHR-ROM banks
    // PPU $1000..=$1FFF Two 4K switchable CHR-ROM banks
    // CPU $6000..=$7FFF 8K PRG-RAM bank
    // CPU $8000..=$BFFF 16K switchable PRG-ROM bank
    // CPU $C000..=$FFFF 16K PRG-ROM bank, fixed to the last bank

    fn map_read(&mut self, addr: u16) -> MappedRead {
        let val = self.map_peek(addr);
        // Update latch after read
        match addr {
            0x0FD8..=0x0FDF | 0x0FE8..=0xFEF | 0x1FD8..=0x1FDF | 0x1FE8..=0x1FEF => {
                let addr = addr as usize;
                self.latch[addr >> 12] = ((addr >> 4) & 0xFF) - 0xFD;
                self.update_banks();
            }
            _ => (),
        }
        val
    }

    fn map_peek(&self, addr: u16) -> MappedRead {
        match addr {
            0x0000..=0x1FFF => MappedRead::Chr(self.chr_banks.translate(addr)),
            0x6000..=0x7FFF => MappedRead::PrgRam((addr & 0x1FFF).into()),
            0x8000..=0xFFFF => MappedRead::PrgRom(self.prg_rom_banks.translate(addr)),
            _ => MappedRead::Bus,
        }
    }

    fn map_write(&mut self, addr: u16, val: u8) -> MappedWrite {
        match addr {
            0x6000..=0x7FFF => MappedWrite::PrgRam((addr & 0x1FFF).into(), val),
            0xA000..=0xAFFF => {
                self.prg_rom_banks.set(0, (val & 0x0F).into());
                MappedWrite::Bus
            }
            0xB000..=0xEFFF => {
                self.latch_banks[((addr - 0xB000) >> 12) as usize] = val & 0x1F;
                self.update_banks();
                MappedWrite::Bus
            }
            0xF000..=0xFFFF => {
                self.mirroring = match val & Self::MIRRORING_MASK {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    _ => unreachable!("impossible mirroring mode"),
                };
                MappedWrite::Bus
            }
            _ => MappedWrite::Bus,
        }
    }
}

impl Reset for Fxrom {
    fn reset(&mut self, _kind: ResetKind) {
        self.latch = [0x00; 2];
        self.latch_banks = [0x00; 4];
        self.update_banks();
    }
}

impl Clock for Fxrom {}
impl Regional for Fxrom {}
