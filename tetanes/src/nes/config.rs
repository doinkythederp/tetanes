use super::{
    action::Action,
    event::{EmulationEvent, RendererEvent},
    input::{Input, InputBinding, InputMap},
    Nes,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf, str::FromStr};
use tetanes_core::{
    apu::Apu, common::NesRegion, control_deck::Config as DeckConfig, fs, input::Player, ppu::Ppu,
    time::Duration,
};
use thiserror::Error;
use tracing::{error, info};
use winit::dpi::LogicalSize;

/// NES emulation configuration settings.
///
/// # Config JSON
///
/// Configuration for `TetaNES` is stored (by default) in `~/.config/tetanes/config.json`
/// with defaults that can be customized in the `TetaNES` config menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[must_use]
#[serde(default)] // Ensures new fields don't break existing configurations
pub struct Config {
    pub audio_enabled: bool,
    pub audio_buffer_size: usize,
    pub audio_latency: Duration,
    pub audio_sample_rate: f32,
    pub concurrent_dpad: bool,
    pub controller_deadzone: f64,
    pub debug: bool,
    pub deck: DeckConfig,
    pub frame_rate: FrameRate,
    pub frame_speed: FrameSpeed,
    pub fullscreen: bool,
    pub hide_overscan: bool,
    pub input_bindings: Vec<InputBinding>,
    #[serde(skip)]
    pub input_map: InputMap,
    pub replay_path: Option<PathBuf>,
    pub rewind: bool,
    pub rewind_buffer_size_mb: usize,
    pub rewind_interval: u8,
    pub rom_path: PathBuf,
    pub recent_roms: HashSet<PathBuf>,
    pub scale: Scale,
    pub show_fps: bool,
    pub show_hidden_files: bool,
    pub show_messages: bool,
    #[serde(skip)]
    pub target_frame_duration: Duration,
    pub threaded: bool,
    pub vsync: bool,
}

impl Default for Config {
    fn default() -> Self {
        let frame_rate = FrameRate::default();
        let input_map = InputMap::default();
        let input_bindings = input_map
            .iter()
            .map(|(input, (slot, action))| (*input, *slot, *action))
            .collect();
        Self {
            audio_buffer_size: if cfg!(target_arch = "wasm32") {
                // Too low a value for wasm causes audio underruns in Chrome
                2048
            } else {
                512
            },
            audio_latency: Duration::from_millis(50),
            audio_enabled: true,
            audio_sample_rate: Apu::DEFAULT_SAMPLE_RATE,
            concurrent_dpad: false,
            controller_deadzone: 0.5,
            debug: false,
            deck: DeckConfig::default(),
            frame_rate,
            frame_speed: FrameSpeed::default(),
            fullscreen: false,
            hide_overscan: true,
            input_map,
            input_bindings,
            replay_path: None,
            rewind: false,
            rewind_buffer_size_mb: 20 * 1024 * 1024,
            rewind_interval: 2,
            rom_path: PathBuf::from("./"),
            recent_roms: HashSet::new(),
            scale: Scale::default(),
            show_fps: cfg!(debug_assertions),
            show_hidden_files: false,
            show_messages: true,
            target_frame_duration: frame_rate.duration(),
            threaded: true,
            vsync: true,
        }
    }
}

impl From<Config> for DeckConfig {
    fn from(config: Config) -> Self {
        config.deck
    }
}

impl Config {
    pub const WINDOW_TITLE: &'static str = "TetaNES";
    pub const DEFAULT_DIRECTORY: &'static str = DeckConfig::DIR;
    pub const FILENAME: &'static str = "config.json";

    pub fn save(&self) -> anyhow::Result<()> {
        if !self.deck.save_on_exit {
            return Ok(());
        }

        let path = self.deck.dir.join(Self::FILENAME);
        let data = serde_json::to_vec_pretty(&self).context("failed to serialize config")?;
        fs::save_raw(path, &data).context("failed to save config")?;

        info!("Saved configuration");
        Ok(())
    }

    pub fn load(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(|| DeckConfig::default().dir().join(Self::FILENAME));
        let mut config = if path.exists() {
            info!("Loading saved configuration");
            fs::load_raw(&path)
                .context("failed to load config")
                .and_then(|data| Ok(serde_json::from_slice::<Config>(&data)?))
                .with_context(|| format!("failed to parse {path:?}"))
                .unwrap_or_else(|err| {
                    error!("Invalid config: {path:?}, reverting to defaults. Error: {err:?}",);
                    Self::default()
                })
        } else {
            info!("Loading default configuration");
            Self::default()
        };

        config.input_map = InputMap::from_bindings(&config.input_bindings);
        let region = config.deck.region;
        Self::set_region(&mut config, region);

        config
    }

    pub fn set_binding(&mut self, input: Input, slot: Player, action: Action) {
        self.input_bindings.push((input, slot, action));
        self.input_map.insert(input, (slot, action));
    }

    pub fn unset_binding(&mut self, input: Input) {
        self.input_bindings.retain(|(i, ..)| i != &input);
        self.input_map.remove(&input);
    }

    pub fn set_region(&mut self, region: NesRegion) {
        self.frame_rate = FrameRate::from(region);
        self.target_frame_duration = Duration::from_secs_f32(
            (f32::from(self.frame_rate) * f32::from(self.frame_speed)).recip(),
        );
        info!(
            "Updated frame rate based on NES Region: {region:?} ({:?}Hz)",
            self.frame_rate,
        );
    }

    pub fn set_frame_speed(&mut self, speed: FrameSpeed) {
        self.frame_speed = speed;
        self.target_frame_duration = Duration::from_secs_f32(
            (f32::from(self.frame_rate) * f32::from(self.frame_speed)).recip(),
        );
    }

    #[must_use]
    pub fn window_size(&self) -> LogicalSize<f32> {
        let scale = f32::from(self.scale);
        let aspect_ratio = self.deck.region.aspect_ratio();
        let (width, height) = self.texture_dimensions();
        LogicalSize::new(scale * width * aspect_ratio, scale * height)
    }

    #[must_use]
    pub fn texture_dimensions(&self) -> (f32, f32) {
        let width = Ppu::WIDTH;
        let height = if self.hide_overscan {
            Ppu::HEIGHT - 16
        } else {
            Ppu::HEIGHT
        };
        (width as f32, height as f32)
    }
}

impl Nes {
    pub fn set_region(&mut self, region: NesRegion) {
        self.config.set_region(region);
        self.trigger_event(EmulationEvent::SetRegion(region));
        self.trigger_event(EmulationEvent::SetTargetFrameDuration(
            self.config.target_frame_duration,
        ));
        self.add_message(format!("Changed NES Region to {region:?}"));
    }

    pub fn set_scale(&mut self, scale: Scale) {
        self.config.scale = scale;
        self.trigger_event(RendererEvent::SetScale(scale));
        self.add_message(format!("Changed Scale to {scale}"));
    }

    pub fn set_speed(&mut self, speed: FrameSpeed) {
        self.config.set_frame_speed(speed);
        self.trigger_event(EmulationEvent::SetFrameSpeed(speed));
        self.trigger_event(EmulationEvent::SetTargetFrameDuration(
            self.config.target_frame_duration,
        ));
        self.add_message(format!("Changed Emulation Speed to {speed}"));
    }
}

#[derive(Error, Debug)]
#[must_use]
#[error("unsupported scale `{0}`. valid values: `1`, `2`, `3`, or `4`")]
pub struct ParseScaleError(String);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum Scale {
    X1,
    X2,
    X3,
    X4,
}

impl Scale {
    pub fn from_str_f32(s: &str) -> anyhow::Result<f32> {
        Ok(f32::from(Self::from_str(s)?))
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::X3
    }
}

impl From<Scale> for f32 {
    fn from(val: Scale) -> Self {
        match val {
            Scale::X1 => 1.0,
            Scale::X2 => 2.0,
            Scale::X3 => 3.0,
            Scale::X4 => 4.0,
        }
    }
}

impl From<&Scale> for f32 {
    fn from(val: &Scale) -> Self {
        Self::from(*val)
    }
}

impl From<Scale> for f64 {
    fn from(val: Scale) -> Self {
        f32::from(val) as f64
    }
}

impl From<&Scale> for f64 {
    fn from(val: &Scale) -> Self {
        Self::from(*val)
    }
}

impl TryFrom<f32> for Scale {
    type Error = ParseScaleError;
    fn try_from(val: f32) -> Result<Self, Self::Error> {
        match val {
            1.0 => Ok(Scale::X1),
            2.0 => Ok(Scale::X2),
            3.0 => Ok(Scale::X3),
            4.0 => Ok(Scale::X4),
            _ => Err(ParseScaleError(val.to_string())),
        }
    }
}

impl AsRef<str> for Scale {
    fn as_ref(&self) -> &str {
        match self {
            Self::X1 => "100%",
            Self::X2 => "200%",
            Self::X3 => "300%",
            Self::X4 => "400%",
        }
    }
}

impl FromStr for Scale {
    type Err = ParseScaleError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let speed = s
            .parse::<f32>()
            .map_err(|_| ParseScaleError(s.to_string()))?;
        Scale::try_from(speed)
    }
}

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

#[derive(Error, Debug)]
#[must_use]
#[error("unsupported frame speed `{0}`. valid values: `.25`, `.50`, `.75`, `1`, `1.25`, `1.50`, `1.75`, or `2`")]
pub struct ParseFrameSpeedError(String);

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum FrameSpeed {
    X25,
    X50,
    X75,
    #[default]
    X100,
    X125,
    X150,
    X175,
    X200,
}

impl FrameSpeed {
    pub fn from_str_f32(s: &str) -> anyhow::Result<f32> {
        Ok(f32::from(Self::from_str(s)?))
    }

    pub fn increment(&self) -> Self {
        match self {
            FrameSpeed::X25 => FrameSpeed::X50,
            FrameSpeed::X50 => FrameSpeed::X75,
            FrameSpeed::X75 => FrameSpeed::X100,
            FrameSpeed::X100 => FrameSpeed::X125,
            FrameSpeed::X125 => FrameSpeed::X150,
            FrameSpeed::X150 => FrameSpeed::X175,
            FrameSpeed::X175 => FrameSpeed::X200,
            FrameSpeed::X200 => FrameSpeed::X200,
        }
    }

    pub fn decrement(&self) -> Self {
        match self {
            FrameSpeed::X25 => FrameSpeed::X25,
            FrameSpeed::X50 => FrameSpeed::X25,
            FrameSpeed::X75 => FrameSpeed::X50,
            FrameSpeed::X100 => FrameSpeed::X75,
            FrameSpeed::X125 => FrameSpeed::X100,
            FrameSpeed::X150 => FrameSpeed::X125,
            FrameSpeed::X175 => FrameSpeed::X150,
            FrameSpeed::X200 => FrameSpeed::X175,
        }
    }
}

impl From<FrameSpeed> for f32 {
    fn from(speed: FrameSpeed) -> Self {
        match speed {
            FrameSpeed::X25 => 0.25,
            FrameSpeed::X50 => 0.50,
            FrameSpeed::X75 => 0.75,
            FrameSpeed::X100 => 1.0,
            FrameSpeed::X125 => 1.25,
            FrameSpeed::X150 => 1.50,
            FrameSpeed::X175 => 1.75,
            FrameSpeed::X200 => 2.0,
        }
    }
}

impl From<&FrameSpeed> for f32 {
    fn from(speed: &FrameSpeed) -> Self {
        Self::from(*speed)
    }
}

impl TryFrom<f32> for FrameSpeed {
    type Error = ParseFrameSpeedError;
    fn try_from(val: f32) -> Result<Self, Self::Error> {
        match val {
            0.25 => Ok(FrameSpeed::X25),
            0.50 => Ok(FrameSpeed::X50),
            0.75 => Ok(FrameSpeed::X75),
            1.0 => Ok(FrameSpeed::X100),
            1.25 => Ok(FrameSpeed::X125),
            1.50 => Ok(FrameSpeed::X150),
            1.75 => Ok(FrameSpeed::X175),
            2.0 => Ok(FrameSpeed::X200),
            _ => Err(ParseFrameSpeedError(val.to_string())),
        }
    }
}

impl AsRef<str> for FrameSpeed {
    fn as_ref(&self) -> &str {
        match self {
            Self::X25 => "25%",
            Self::X50 => "50%",
            Self::X75 => "75%",
            Self::X100 => "100%",
            Self::X125 => "125%",
            Self::X150 => "150%",
            Self::X175 => "175%",
            Self::X200 => "200%",
        }
    }
}

impl FromStr for FrameSpeed {
    type Err = ParseFrameSpeedError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let speed = s
            .parse::<f32>()
            .map_err(|_| ParseFrameSpeedError(s.to_string()))?;
        FrameSpeed::try_from(speed)
    }
}

impl std::fmt::Display for FrameSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

#[derive(Error, Debug)]
#[must_use]
#[error("unsupported sample rate `{0}`. valid values: `44100` or `48000`")]
pub struct ParseSampleRateError(u32);

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SampleRate {
    #[default]
    S44,
    S48,
}

impl SampleRate {
    pub const MIN: Self = Self::S44;
    pub const MAX: Self = Self::S48;
}

impl From<SampleRate> for u32 {
    fn from(sample_rate: SampleRate) -> Self {
        match sample_rate {
            SampleRate::S44 => 44100,
            SampleRate::S48 => 48000,
        }
    }
}

impl From<&SampleRate> for u32 {
    fn from(sample_rate: &SampleRate) -> Self {
        Self::from(*sample_rate)
    }
}

impl From<SampleRate> for f32 {
    fn from(sample_rate: SampleRate) -> Self {
        u32::from(sample_rate) as f32
    }
}

impl From<&SampleRate> for f32 {
    fn from(sample_rate: &SampleRate) -> Self {
        Self::from(*sample_rate)
    }
}

impl TryFrom<u32> for SampleRate {
    type Error = ParseSampleRateError;
    fn try_from(val: u32) -> Result<Self, Self::Error> {
        match val {
            44100 => Ok(Self::S44),
            48000 => Ok(Self::S48),
            _ => Err(ParseSampleRateError(val)),
        }
    }
}

impl AsRef<str> for SampleRate {
    fn as_ref(&self) -> &str {
        match self {
            Self::S44 => "44.1 kHz",
            Self::S48 => "48 kHz",
        }
    }
}

impl std::fmt::Display for SampleRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

#[derive(Error, Debug)]
#[must_use]
#[error("unsupported frame rate `{0}`. valid values: `50`, `59`, or `60`")]
pub struct ParseFrameRateError(u32);

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FrameRate {
    X50,
    X59,
    #[default]
    X60,
}

impl FrameRate {
    pub const MIN: Self = Self::X50;
    pub const MAX: Self = Self::X60;

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(f32::from(self).recip())
    }
}

impl From<FrameRate> for u32 {
    fn from(frame_rate: FrameRate) -> Self {
        match frame_rate {
            FrameRate::X50 => 50,
            FrameRate::X59 => 59,
            FrameRate::X60 => 60,
        }
    }
}

impl From<&FrameRate> for u32 {
    fn from(frame_rate: &FrameRate) -> Self {
        Self::from(*frame_rate)
    }
}

impl From<FrameRate> for f32 {
    fn from(frame_rate: FrameRate) -> Self {
        u32::from(frame_rate) as f32
    }
}

impl From<&FrameRate> for f32 {
    fn from(frame_rate: &FrameRate) -> Self {
        Self::from(*frame_rate)
    }
}

impl TryFrom<u32> for FrameRate {
    type Error = ParseFrameRateError;
    fn try_from(val: u32) -> Result<Self, Self::Error> {
        match val {
            50 => Ok(Self::X50),
            59 => Ok(Self::X59),
            60 => Ok(Self::X60),
            _ => Err(ParseFrameRateError(val)),
        }
    }
}

impl From<NesRegion> for FrameRate {
    fn from(region: NesRegion) -> Self {
        match region {
            NesRegion::Pal => Self::X50,
            NesRegion::Dendy => Self::X59,
            NesRegion::Ntsc => Self::X60,
        }
    }
}

impl From<&NesRegion> for FrameRate {
    fn from(region: &NesRegion) -> Self {
        Self::from(*region)
    }
}

impl AsRef<str> for FrameRate {
    fn as_ref(&self) -> &str {
        match self {
            Self::X50 => "50 Hz",
            Self::X59 => "59 Hz",
            Self::X60 => "60 Hz",
        }
    }
}

impl std::fmt::Display for FrameRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}
