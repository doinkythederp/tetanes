use crate::{
    common::Powered,
    ppu::{RENDER_HEIGHT, RENDER_SIZE, RENDER_WIDTH},
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{f64::consts::PI, fmt};

lazy_static! {
    pub static ref NTSC_PALETTE: Vec<Vec<Vec<u32>>> = {
        // NOTE: There's lot's to clean up here -- too many magic numbers and duplication but
        // I'm afraid to touch it now that it works
        // Source: https://bisqwit.iki.fi/jutut/kuvat/programming_examples/nesemu1/nesemu1.cc
        // http://wiki.nesdev.com/w/index.php/NTSC_video

        // Calculate the luma and chroma by emulating the relevant circuits:
        const VOLTAGES: [i32; 16] = [
            -6, -69, 26, -59, 29, -55, 73, -40, 68, -17, 125, 11, 68, 33, 125, 78,
        ];

        let mut ntsc_palette = vec![vec![vec![0; 512]; 64]; 3];

        // Helper functions for converting YIQ to RGB
        let gamma = 2.0; // Assumed display gamma
        let gammafix = |color: f64| {
            if color <= 0.0 {
                0.0
            } else {
                color.powf(2.2 / gamma)
            }
        };
        let yiq_divider = f64::from(9 * 10u32.pow(6));
        for (palette_offset, palette) in ntsc_palette.iter_mut().enumerate() {
            for channel in 0..3 {
                for color0_offset in 0..512 {
                    let emphasis = color0_offset / 64;

                    for (color1_offset, color1) in palette.iter_mut().enumerate() {
                        let mut y = 0;
                        let mut i = 0;
                        let mut q = 0;
                        // 12 samples of NTSC signal constitute a color.
                        for sample in 0..12 {
                            let noise = (sample + palette_offset * 4) % 12;
                            // Sample either the previous or the current pixel.
                            // Use pixel=color0 to disable artifacts.
                            let pixel = if noise < 6 - channel * 2 {
                                color0_offset
                            } else {
                                color1_offset
                            };

                            // Decode the color index.
                            let chroma = pixel & 0x0F;
                            // Forces luma to 0, 4, 8, or 12 for easy lookup
                            let luma = if chroma < 0x0E { (pixel / 4) & 12 } else { 4 };
                            // NES NTSC modulator (square wave between up to four voltage levels):
                            let limit = if (chroma + 8 + sample) % 12 < 6 {
                                12
                            } else {
                                0
                            };
                            let high = if chroma > limit { 1 } else { 0 };
                            let emp_effect = if (152_278 >> (sample / 2 * 3)) & emphasis > 0 {
                                0
                            } else {
                                2
                            };
                            let level = 40 + VOLTAGES[high + emp_effect + luma];
                            // Ideal TV NTSC demodulator:
                            let (sin, cos) = (PI * sample as f64 / 6.0).sin_cos();
                            y += level;
                            i += level * (cos * 5909.0) as i32;
                            q += level * (sin * 5909.0) as i32;
                        }
                        // Store color at subpixel precision
                        let y = f64::from(y) / 1980.0;
                        let i = f64::from(i) / yiq_divider;
                        let q = f64::from(q) / yiq_divider;
                        match channel {
                            2 => {
                                let rgb = 255.95 * gammafix(q.mul_add(0.623_557, i.mul_add(0.946_882, y)));
                                color1[color0_offset] += 0x10000 * rgb.clamp(0.0, 255.0) as u32;
                            }
                            1 => {
                                let rgb = 255.95 * gammafix(q.mul_add(-0.635_691, i.mul_add(-0.274_788, y)));
                                color1[color0_offset] += 0x00100 * rgb.clamp(0.0, 255.0) as u32;
                            }
                            0 => {
                                let rgb = 255.95 * gammafix(q.mul_add(1.709_007, i.mul_add(-1.108_545, y)));
                                color1[color0_offset] += rgb.clamp(0.0, 255.0) as u32;
                            }
                            _ => (), // invalid channel
                        }
                    }
                }
            }
        }

        ntsc_palette
    };
}

#[derive(Clone, Serialize, Deserialize)]
#[must_use]
pub struct Frame {
    pub num: u32,
    // Shift registers
    pub tile_lo: u8,
    pub tile_hi: u8,
    // Tile data - stored in cycles 0 mod 8
    pub nametable: u16,
    pub attribute: u8,
    pub tile_data: u64,
    pub prev_pixel: u32,
    pub last_updated_pixel: u32,
    pub current_buffer: Vec<u16>,
    pub back_buffer: Vec<u16>,
    pub output_buffer: Vec<u8>,
}

impl Frame {
    pub fn new() -> Self {
        Self {
            num: 0,
            nametable: 0,
            attribute: 0,
            tile_lo: 0,
            tile_hi: 0,
            tile_data: 0,
            prev_pixel: 0xFFFF_FFFF,
            last_updated_pixel: 0,
            current_buffer: vec![0; (RENDER_WIDTH * RENDER_HEIGHT) as usize],
            back_buffer: vec![0; (RENDER_WIDTH * RENDER_HEIGHT) as usize],
            output_buffer: vec![0; RENDER_SIZE],
        }
    }

    #[inline]
    pub fn increment(&mut self) {
        self.num += 1;
    }

    #[inline]
    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.current_buffer, &mut self.back_buffer);
    }

    #[inline]
    pub fn put_pixel(&mut self, x: u32, y: u32, color: u16) {
        self.back_buffer[(x + (y << 8)) as usize] = color;
    }
}

impl Powered for Frame {
    fn reset(&mut self) {
        self.num = 0;
        self.current_buffer.fill(0);
        self.back_buffer.fill(0);
        self.output_buffer.fill(0);
    }
    fn power_cycle(&mut self) {
        self.reset();
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("num", &self.num)
            .field("tile_lo", &format_args!("${:02X}", &self.tile_lo))
            .field("tile_hi", &format_args!("${:02X}", &self.tile_hi))
            .field("nametable", &format_args!("${:04X}", &self.nametable))
            .field("attribute", &format_args!("${:02X}", &self.attribute))
            .field("tile_data", &format_args!("${:16X}", &self.tile_data))
            .field("prev_pixel", &self.prev_pixel)
            .finish()
    }
}
