//! PPU frame implementation.

use crate::{
    common::{Reset, ResetKind},
    ppu::Ppu,
};
use alloc::{vec, vec::Vec};
use core::ops::{Deref, DerefMut};
use serde::{Deserialize, Serialize};

/// PPU frame.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
#[must_use]
pub struct Buffer(Vec<u16>);

impl core::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Buffer({} elements)", self.0.len())
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self(vec![0x00; Ppu::SIZE])
    }
}

impl Deref for Buffer {
    type Target = [u16];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// PPU frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Frame {
    pub count: u32,
    #[serde(skip)]
    pub buffer: Buffer,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    pub fn new() -> Self {
        Self {
            count: 0,
            buffer: Buffer::default(),
        }
    }

    pub fn increment(&mut self) {
        self.count = self.count.wrapping_add(1);
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> u16 {
        self.buffer[(x + (y << 8)) as usize]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: u16) {
        self.buffer[(x + (y << 8)) as usize] = color;
    }

    #[must_use]
    pub fn pixel_brightness(&self, x: u32, y: u32) -> u32 {
        let pixel = self.pixel(x, y);
        let (red, green, blue) = Ppu::system_palette(pixel);
        u32::from(red) + u32::from(green) + u32::from(blue)
    }

    #[must_use]
    pub const fn number(&self) -> u32 {
        self.count
    }

    #[must_use]
    pub fn buffer(&self) -> &[u16] {
        &self.buffer
    }
}

impl Reset for Frame {
    fn reset(&mut self, _kind: ResetKind) {
        self.count = 0;
        self.buffer.fill(0);
    }
}
