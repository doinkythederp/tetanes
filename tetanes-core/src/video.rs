//! Video output and filtering.

use crate::ppu::Ppu;
use alloc::{vec, vec::Vec};
use core::{
    f64::consts::PI,
    ops::{Deref, DerefMut},
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[must_use]
pub enum VideoFilter {
    Pixellate,
    #[default]
    Ntsc,
}

impl VideoFilter {
    pub const fn as_slice() -> &'static [Self] {
        &[Self::Pixellate, Self::Ntsc]
    }
}

impl AsRef<str> for VideoFilter {
    fn as_ref(&self) -> &str {
        match self {
            Self::Pixellate => "Pixellate",
            Self::Ntsc => "NTSC",
        }
    }
}

impl From<usize> for VideoFilter {
    fn from(value: usize) -> Self {
        if value == 1 {
            Self::Ntsc
        } else {
            Self::Pixellate
        }
    }
}

#[derive(Debug, Clone)]
#[must_use]
pub struct Frame(Vec<u8>);

impl Frame {
    pub const SIZE: usize = Ppu::SIZE * 4;

    /// Allocate a new frame for video output.
    pub fn new() -> Self {
        let mut frame = vec![0; Self::SIZE];
        frame
            .iter_mut()
            .skip(3)
            .step_by(4)
            .for_each(|alpha| *alpha = 255);
        Self(frame)
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for Frame {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Frame {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone)]
#[must_use]
pub struct Video {
    pub filter: VideoFilter,
    pub frame: Frame,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

impl Video {
    /// Create a new Video decoder with the default filter.
    pub fn new() -> Self {
        Self::with_filter(VideoFilter::default())
    }

    /// Create a new Video encoder with a filter.
    pub fn with_filter(filter: VideoFilter) -> Self {
        Self {
            filter,
            frame: Frame::new(),
        }
    }

    /// Applies the given filter to the given video buffer and returns the result.
    pub fn apply_filter(&mut self, buffer: &[u16], frame_number: u32) -> &[u8] {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        match self.filter {
            VideoFilter::Pixellate => Self::decode_buffer(buffer, &mut self.frame),
            VideoFilter::Ntsc => Self::apply_ntsc_filter(buffer, frame_number, &mut self.frame),
        }

        &self.frame
    }

    /// Applies the given filter to the given video buffer by coping into the provided buffer.
    pub fn apply_filter_into(&self, buffer: &[u16], frame_number: u32, output: &mut [u8]) {
        #[cfg(feature = "profiling")]
        puffin::profile_function!();

        match self.filter {
            VideoFilter::Pixellate => Self::decode_buffer(buffer, output),
            VideoFilter::Ntsc => Self::apply_ntsc_filter(buffer, frame_number, output),
        }
    }

    /// Fills a fully rendered frame with RGB colors.
    pub fn decode_buffer(buffer: &[u16], output: &mut [u8]) {
        for (pixel, colors) in buffer.iter().zip(output.chunks_exact_mut(4)) {
            let index = (*pixel as usize) * 3;
            assert!(Ppu::NTSC_PALETTE.len() > index + 2);
            assert!(colors.len() > 2);
            colors[0] = Ppu::NTSC_PALETTE[index];
            colors[1] = Ppu::NTSC_PALETTE[index + 1];
            colors[2] = Ppu::NTSC_PALETTE[index + 2];
        }
    }

    /// Applies the NTSC filter to the given video buffer.
    ///
    /// Amazing implementation Bisqwit! Much faster than my original, but boy what a pain
    /// to translate it to Rust
    /// Source: <https://bisqwit.iki.fi/jutut/kuvat/programming_examples/nesemu1/nesemu1.cc>
    /// See also: <http://wiki.nesdev.com/w/index.php/NTSC_video>
    pub fn apply_ntsc_filter(buffer: &[u16], frame_number: u32, output: &mut [u8]) {
        let mut prev_pixel = 0;
        for (idx, (pixel, colors)) in buffer.iter().zip(output.chunks_exact_mut(4)).enumerate() {
            let x = idx % 256;
            let color = if x == 0 {
                // Remove pixel 0 artifact from not having a valid previous pixel
                0
            } else {
                let y = idx / 256;
                let even_phase = if frame_number & 0x01 == 0x01 { 0 } else { 1 };
                let phase = (2 + y * 341 + x + even_phase) % 3;
                NTSC_PALETTE
                    [phase + ((prev_pixel & 0x3F) as usize) * 3 + (*pixel as usize) * 3 * 64]
            };
            prev_pixel = u32::from(*pixel);
            assert!(colors.len() > 2);
            colors[0] = (color >> 16 & 0xFF) as u8;
            colors[1] = (color >> 8 & 0xFF) as u8;
            colors[2] = (color & 0xFF) as u8;
            // Alpha should always be 255
        }
    }
}

impl core::fmt::Debug for Video {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Video")
            .field("filter", &self.filter)
            .finish()
    }
}

lazy_static! {
    pub static ref NTSC_PALETTE: Vec<u32> = generate_ntsc_palette();
}
fn generate_ntsc_palette() -> Vec<u32> {
    // NOTE: There's lot's to clean up here -- too many magic numbers and duplication but
    // I'm afraid to touch it now that it works
    // Source: https://bisqwit.iki.fi/jutut/kuvat/programming_examples/nesemu1/nesemu1.cc
    // http://wiki.nesdev.com/w/index.php/NTSC_video

    // Calculate the luma and chroma by emulating the relevant circuits:
    const VOLTAGES: [i32; 16] = [
        -6, -69, 26, -59, 29, -55, 73, -40, 68, -17, 125, 11, 68, 33, 125, 78,
    ];

    let mut ntsc_palette = vec![0; 512 * 64 * 3];

    // Helper functions for converting YIQ to RGB
    let gamma = 2.0; // Assumed display gamma
    let gammafix = |color: f64| {
        if color <= 0.0 {
            0.0
        } else {
            libm::pow(color, 2.2 / gamma)
        }
    };
    let yiq_divider = f64::from(9 * 10u32.pow(6));
    for palette_offset in 0..3 {
        for channel in 0..3 {
            for color0_offset in 0..512 {
                let emphasis = color0_offset / 64;

                for color1_offset in 0..64 {
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
                        let (sin, cos) = libm::sincos(PI * sample as f64 / 6.0);
                        y += level;
                        i += level * (cos * 5909.0) as i32;
                        q += level * (sin * 5909.0) as i32;
                    }
                    // Store color at subpixel precision
                    let y = f64::from(y) / 1980.0;
                    let i = f64::from(i) / yiq_divider;
                    let q = f64::from(q) / yiq_divider;
                    let idx = palette_offset + color0_offset * 3 * 64 + color1_offset * 3;
                    match channel {
                        2 => {
                            let rgb = 255.95
                                * gammafix(libm::fma(q, 0.623_557, libm::fma(i, 0.946_882, y)));
                            ntsc_palette[idx] += 0x10000 * rgb.clamp(0.0, 255.0) as u32;
                        }
                        1 => {
                            let rgb = 255.95
                                * gammafix(libm::fma(q, -0.635_691, libm::fma(i, -0.274_788, y)));
                            ntsc_palette[idx] += 0x00100 * rgb.clamp(0.0, 255.0) as u32;
                        }
                        0 => {
                            let rgb = 255.95
                                * gammafix(libm::fma(q, 1.709_007, libm::fma(i, -1.108_545, y)));
                            ntsc_palette[idx] += rgb.clamp(0.0, 255.0) as u32;
                        }
                        _ => (), // invalid channel
                    }
                }
            }
        }
    }

    ntsc_palette
}
