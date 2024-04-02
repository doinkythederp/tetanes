use crate::{
    apu::{length_counter::LengthCounter, linear_counter::LinearCounter},
    common::{Clock, Reset, ResetKind, Sample},
};
use serde::{Deserialize, Serialize};

/// APU Triangle Channel provides triangle wave generation.
///
/// See: <https://www.nesdev.org/wiki/APU_Triangle>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Triangle {
    pub force_silent: bool,
    pub ultrasonic: bool,
    pub step: u8,
    pub freq_timer: u16,
    pub freq_counter: u16,
    pub length: LengthCounter,
    pub linear: LinearCounter,
}

impl Default for Triangle {
    fn default() -> Self {
        Self::new()
    }
}

impl Triangle {
    pub const fn new() -> Self {
        Self {
            force_silent: false,
            ultrasonic: false,
            step: 0u8,
            freq_timer: 0u16,
            freq_counter: 0u16,
            length: LengthCounter::new(),
            linear: LinearCounter::new(),
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

    pub fn clock_quarter_frame(&mut self) {
        if self.linear.reload {
            self.linear.counter = self.linear.load;
        } else if self.linear.counter > 0 {
            self.linear.counter -= 1;
        }
        if !self.linear.control {
            self.linear.reload = false;
        }
    }

    pub fn clock_half_frame(&mut self) {
        self.length.clock();
    }

    /// $4008 Linear counter control
    pub fn write_linear_counter(&mut self, val: u8) {
        self.linear.control = (val & 0x80) == 0x80; // D7
        self.linear.load_value(val);
        self.length.write_ctrl(self.linear.control); // !D7
    }

    /// $400A Triangle timer lo
    pub fn write_timer_lo(&mut self, val: u8) {
        self.freq_timer = (self.freq_timer & 0xFF00) | u16::from(val); // D7..D0
    }

    /// $400B Triangle timer high
    pub fn write_timer_hi(&mut self, val: u8) {
        self.freq_timer = (self.freq_timer & 0x00FF) | u16::from(val & 0x07) << 8; // D2..D0
        self.freq_counter = self.freq_timer;
        self.linear.reload = true;
        self.length.write(val);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.length.set_enabled(enabled);
    }
}

impl Sample for Triangle {
    #[must_use]
    fn output(&self) -> f32 {
        if self.force_silent {
            0.0
        } else if self.freq_timer < 2 {
            7.5
        } else if self.step & 0x10 == 0x10 {
            f32::from(self.step ^ 0x1F)
        } else {
            f32::from(self.step)
        }
    }
}

impl Clock for Triangle {
    fn clock(&mut self) -> usize {
        if self.linear.counter > 0 && self.length.counter > 0 {
            if self.freq_counter > 0 {
                self.freq_counter -= 1;
            } else {
                self.freq_counter = self.freq_timer;
                self.step = (self.step + 1) & 0x1F;
            }
            1
        } else {
            0
        }
    }
}

impl Reset for Triangle {
    fn reset(&mut self, kind: ResetKind) {
        self.ultrasonic = false;
        self.step = 0u8;
        self.freq_timer = 0u16;
        self.freq_counter = 0u16;
        self.length.reset(kind);
        self.linear = LinearCounter::new();
    }
}
