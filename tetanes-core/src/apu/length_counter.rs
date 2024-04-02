use crate::common::{Clock, Reset, ResetKind};
use serde::{Deserialize, Serialize};

/// APU Length Counter provides duration control for APU waveform channels.
///
/// See: <https://www.nesdev.org/wiki/APU_Length_Counter>
#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
pub struct LengthCounter {
    pub enabled: bool,
    pub halt: bool,
    pub new_halt: bool,
    pub counter: u8, // Entry into LENGTH_TABLE
    pub previous_counter: u8,
    pub reload: u8,
}

impl LengthCounter {
    const LENGTH_TABLE: [u8; 32] = [
        10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96,
        22, 192, 24, 72, 26, 16, 28, 32, 30,
    ];

    pub const fn new() -> Self {
        Self {
            enabled: false,
            halt: false,
            new_halt: false,
            counter: 0,
            previous_counter: 0,
            reload: 0,
        }
    }

    #[inline]
    pub fn write(&mut self, val: u8) {
        if self.enabled {
            self.reload = Self::LENGTH_TABLE[(val >> 3) as usize]; // D7..D3
            self.previous_counter = self.counter;
            // TODO: set apu needs to run
        }
    }

    #[inline]
    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.counter = 0;
        }
        self.enabled = enabled;
    }

    #[inline]
    pub fn reload(&mut self) {
        if self.reload > 0 {
            if self.counter == self.previous_counter {
                self.counter = self.reload;
            }
            self.reload = 0;
        }
        self.halt = self.new_halt;
    }

    #[inline]
    pub fn write_ctrl(&mut self, halt: bool) {
        // TODO: set apu needs to run
        self.new_halt = halt; // !D5
    }
}

impl Clock for LengthCounter {
    fn clock(&mut self) -> usize {
        if self.counter > 0 && !self.halt {
            self.counter -= 1;
            1
        } else {
            0
        }
    }
}

impl Reset for LengthCounter {
    fn reset(&mut self, kind: ResetKind) {
        self.enabled = false;
        match kind {
            ResetKind::Soft => {
                // TODO: if not triangle
                self.halt = false;
                self.new_halt = false;
                self.counter = 0;
                self.previous_counter = 0;
                self.reload = 0;
            }
            ResetKind::Hard => {
                self.halt = false;
                self.new_halt = false;
                self.counter = 0;
                self.previous_counter = 0;
                self.reload = 0;
            }
        }
        self.reload();
    }
}
