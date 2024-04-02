use crate::{
    apu::{envelope::Envelope, length_counter::LengthCounter},
    common::{Clock, NesRegion, Regional, Reset, ResetKind, Sample},
};
use serde::{Deserialize, Serialize};

/// Noise shift mode.
#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
pub enum ShiftMode {
    /// Zero (XOR bits 0 and 1)
    Zero,
    /// One (XOR bits 0 and 6)
    One,
}

/// APU Noise Channel provides pseudo-random noise generation.
///
/// See: <https://www.nesdev.org/wiki/APU_Noise>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Noise {
    pub region: NesRegion,
    pub force_silent: bool,
    pub freq_timer: u16,   // timer freq_counter reload value
    pub freq_counter: u16, // Current frequency timer value
    pub shift: u16,        // Must never be 0
    pub shift_mode: ShiftMode,
    pub length: LengthCounter,
    pub envelope: Envelope,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    const FREQ_TABLE_NTSC: [u16; 16] = [
        4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
    ];
    const FREQ_TABLE_PAL: [u16; 16] = [
        4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778,
    ];
    const SHIFT_BIT_15_MASK: u16 = !0x8000;

    pub fn new() -> Self {
        Self {
            region: NesRegion::default(),
            force_silent: false,
            freq_timer: 0u16,
            freq_counter: 0u16,
            shift: 1u16, // Must never be 0
            shift_mode: ShiftMode::Zero,
            length: LengthCounter::new(),
            envelope: Envelope::new(),
        }
    }

    #[must_use]
    pub const fn silent(&self) -> bool {
        self.force_silent
    }

    pub fn toggle_silent(&mut self) {
        self.force_silent = !self.force_silent;
    }

    #[must_use]
    pub const fn length_counter(&self) -> u8 {
        self.length.counter
    }

    const fn freq_timer(region: NesRegion, val: u8) -> u16 {
        match region {
            NesRegion::Ntsc => Self::FREQ_TABLE_NTSC[(val & 0x0F) as usize] - 1,
            NesRegion::Pal | NesRegion::Dendy => Self::FREQ_TABLE_PAL[(val & 0x0F) as usize] - 1,
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.envelope.clock();
        self.length.clock();
    }

    /// $400C Noise control
    pub fn write_ctrl(&mut self, val: u8) {
        self.length.write_ctrl((val & 0x20) == 0x20); // !D5
        self.envelope.write_ctrl(val);
    }

    /// $400E Noise timer
    pub fn write_timer(&mut self, val: u8) {
        self.freq_timer = Self::freq_timer(self.region, val);
        self.shift_mode = if (val >> 7) & 1 == 1 {
            ShiftMode::One
        } else {
            ShiftMode::Zero
        };
    }

    /// $400F Length counter
    pub fn write_length(&mut self, val: u8) {
        self.length.write(val);
        self.envelope.restart();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.length.set_enabled(enabled);
    }

    fn volume(&self) -> u8 {
        if self.length.counter > 0 {
            self.envelope.volume()
        } else {
            0
        }
    }
}

impl Sample for Noise {
    #[must_use]
    fn output(&self) -> f32 {
        if self.shift & 1 == 1 || self.silent() {
            0f32
        } else {
            f32::from(self.volume())
        }
    }
}

impl Clock for Noise {
    fn clock(&mut self) -> usize {
        if self.freq_counter > 0 {
            self.freq_counter -= 1;
        } else {
            self.freq_counter = self.freq_timer;
            let shift_amount = if self.shift_mode == ShiftMode::One {
                6
            } else {
                1
            };
            let bit1 = self.shift & 1; // Bit 0
            let bit2 = (self.shift >> shift_amount) & 1; // Bit 1 or 6 from above
            self.shift = (self.shift & Self::SHIFT_BIT_15_MASK) | ((bit1 ^ bit2) << 14);
            self.shift >>= 1;
        }
        1
    }
}

impl Regional for Noise {
    fn region(&self) -> NesRegion {
        self.region
    }

    fn set_region(&mut self, region: NesRegion) {
        self.region = region;
    }
}

impl Reset for Noise {
    fn reset(&mut self, kind: ResetKind) {
        self.freq_timer = 0u16;
        self.freq_counter = 0u16;
        self.shift = 1u16;
        self.shift_mode = ShiftMode::Zero;
        self.length.reset(kind);
        self.envelope.reset(kind);
    }
}
