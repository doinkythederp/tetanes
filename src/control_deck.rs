use crate::{
    apu::{Apu, AudioChannel},
    bus::Bus,
    cart::Cart,
    common::{Clocked, Powered},
    cpu::{instr::Instr, Cpu, CPU_CLOCK_RATE},
    debugger::Breakpoint,
    input::{Gamepad, GamepadSlot, Zapper},
    memory::RamState,
    ppu::{Ppu, VideoFormat},
    NesResult,
};
use std::{io::Read, ops::ControlFlow};

/// Represents an NES Control Deck
#[derive(Debug, Clone)]
#[must_use]
pub struct ControlDeck {
    running: bool,
    power_state: RamState,
    loaded_rom: Option<String>,
    turbo_clock: usize,
    cycles_remaining: f32,
    cpu: Cpu,
}

impl ControlDeck {
    /// Creates a new `ControlDeck` instance.
    #[inline]
    pub fn new(power_state: RamState) -> Self {
        let cpu = Cpu::init(Bus::new(power_state));
        Self {
            running: false,
            power_state,
            loaded_rom: None,
            turbo_clock: 0,
            cycles_remaining: 0.0,
            cpu,
        }
    }

    /// Loads a ROM cartridge into memory
    ///
    /// # Errors
    ///
    /// If there is any issue loading the ROM, then an error is returned.
    #[inline]
    pub fn load_rom<S: ToString, F: Read>(&mut self, name: &S, rom: &mut F) -> NesResult<()> {
        self.power_off();
        self.loaded_rom = Some(name.to_string());
        let cart = Cart::from_rom(name, rom, self.power_state)?;
        self.cpu.bus.load_cart(cart);
        self.power_on();
        Ok(())
    }

    #[inline]
    pub fn load_cpu(&mut self, mut cpu: Cpu) {
        // Swap CPU, but keep original loaded cart, except for ram and mapper
        std::mem::swap(&mut self.cpu, &mut cpu);
        std::mem::swap(&mut self.cpu.bus.cart, &mut cpu.bus.cart);
        self.cpu.bus.ppu.load_cart(&mut self.cpu.bus.cart);
        self.cpu.bus.apu.load_cart(&mut self.cpu.bus.cart);
        self.cpu.bus.cart.prg_ram = cpu.bus.cart.prg_ram;
        self.cpu.bus.cart.chr = cpu.bus.cart.chr;
        self.cpu.bus.cart.mapper = cpu.bus.cart.mapper;
    }

    #[inline]
    #[must_use]
    pub const fn loaded_rom(&self) -> &Option<String> {
        &self.loaded_rom
    }

    /// Get a frame worth of pixels.
    #[inline]
    #[must_use]
    pub fn frame(&self) -> &[u8] {
        self.cpu.bus.ppu.frame_buffer()
    }

    /// Get the current frame number.
    #[inline]
    #[must_use]
    pub fn frame_number(&self) -> u32 {
        self.cpu.bus.ppu.frame.num
    }

    /// Get audio samples.
    #[inline]
    #[must_use]
    pub fn audio_samples(&self) -> &[f32] {
        self.cpu.bus.apu.samples()
    }

    /// Clear audio samples.
    #[inline]
    pub fn clear_audio_samples(&mut self) {
        self.cpu.bus.apu.clear_samples();
    }

    /// Set the emulation speed.
    #[inline]
    pub fn set_speed(&mut self, speed: f32) {
        self.cpu.bus.apu.set_speed(speed);
    }

    /// Steps the control deck the number of seconds
    #[inline]
    pub fn clock_seconds(&mut self, seconds: f32) -> usize {
        self.cycles_remaining += CPU_CLOCK_RATE * seconds;
        let mut clocks = 0;
        while self.cycles_remaining > 0.0 && !self.cpu_corrupted() {
            let cycles = self.clock();
            clocks += cycles;
            self.cycles_remaining -= cycles as f32;
        }
        clocks
    }

    #[inline]
    pub(crate) fn debug_clock_frame(
        &mut self,
        breakpoints: &[Breakpoint],
    ) -> ControlFlow<usize, usize> {
        for zapper in &mut self.cpu.bus.input.zappers {
            zapper.update();
        }
        self.clock_turbo();
        let mut clocks = 0;
        while !self.frame_complete() && !self.cpu_corrupted() {
            if breakpoints.iter().any(|bp| bp.matches(&self.cpu)) {
                return ControlFlow::Break(clocks);
            }
            clocks += self.clock();
        }
        self.start_new_frame();
        ControlFlow::Continue(clocks)
    }

    /// Steps the control deck an entire frame
    #[inline]
    pub fn clock_frame(&mut self) -> usize {
        match self.debug_clock_frame(&[]) {
            ControlFlow::Continue(clocks) | ControlFlow::Break(clocks) => clocks,
        }
    }

    /// Steps the control deck a single scanline.
    #[inline]
    pub fn clock_scanline(&mut self) -> usize {
        let current_scanline = self.cpu.bus.ppu.scanline;
        let mut clocks = 0;
        while self.cpu.bus.ppu.scanline == current_scanline && !self.cpu_corrupted() {
            clocks += self.clock();
        }
        clocks
    }

    /// Returns whether the CPU is corrupted or not.
    #[inline]
    #[must_use]
    pub const fn cpu_corrupted(&self) -> bool {
        self.cpu.corrupted
    }

    /// Returns the current CPU program counter.
    #[inline]
    #[must_use]
    pub const fn pc(&self) -> u16 {
        self.cpu.pc
    }

    /// Returns the next CPU instruction to be executed.
    #[inline]
    pub fn next_instr(&self) -> Instr {
        self.cpu.next_instr()
    }

    /// Returns the next address on the bus with the current value at the target address, if
    /// appropriate.
    #[inline]
    #[must_use]
    pub fn next_addr(&self) -> (Option<u16>, Option<u16>) {
        self.cpu.next_addr()
    }

    /// Returns the address at the top of the stack.
    #[inline]
    #[must_use]
    pub fn stack_addr(&self) -> u16 {
        self.cpu.peek_stackw()
    }

    /// Disassemble an address range of CPU instructions.
    #[inline]
    #[must_use]
    pub fn disasm(&self, start: u16, end: u16) -> Vec<String> {
        let mut disassembly = Vec::with_capacity(256);
        let mut addr = start;
        while addr <= end {
            disassembly.push(self.cpu.disassemble(&mut addr));
        }
        disassembly
    }

    #[inline]
    pub const fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    #[inline]
    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    #[inline]
    pub const fn ppu(&self) -> &Ppu {
        &self.cpu.bus.ppu
    }

    #[inline]
    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.cpu.bus.ppu
    }

    #[inline]
    pub const fn apu(&self) -> &Apu {
        &self.cpu.bus.apu
    }

    #[inline]
    pub const fn cart(&self) -> &Cart {
        &self.cpu.bus.cart
    }

    #[inline]
    pub fn cart_mut(&mut self) -> &mut Cart {
        &mut self.cpu.bus.cart
    }

    #[inline]
    #[must_use]
    pub const fn frame_complete(&self) -> bool {
        self.cpu.bus.ppu.frame_complete
    }

    #[inline]
    pub fn start_new_frame(&mut self) {
        self.cpu.bus.ppu.frame_complete = false;
    }

    /// Returns a mutable reference to a gamepad.
    #[inline]
    pub fn gamepad_mut(&mut self, slot: GamepadSlot) -> &mut Gamepad {
        &mut self.cpu.bus.input.gamepads[slot as usize]
    }

    /// Returns a reference to the zapper.
    #[inline]
    pub const fn zapper(&self, slot: GamepadSlot) -> &Zapper {
        &self.cpu.bus.input.zappers[slot as usize]
    }

    /// Returns a mutable reference to the zapper.
    #[inline]
    pub fn zapper_mut(&mut self, slot: GamepadSlot) -> &mut Zapper {
        &mut self.cpu.bus.input.zappers[slot as usize]
    }

    /// Get the video filter for the emulation.
    #[inline]
    pub const fn filter(&self) -> VideoFormat {
        self.cpu.bus.ppu.filter
    }

    /// Set the video filter for the emulation.
    #[inline]
    pub fn set_filter(&mut self, filter: VideoFormat) {
        self.cpu.bus.ppu.filter = filter;
    }

    /// Returns whether a given API audio channel is enabled.
    #[inline]
    pub fn channel_enabled(&mut self, channel: AudioChannel) -> bool {
        self.cpu.bus.apu.channel_enabled(channel)
    }

    /// Toggle one of the APU audio channels.
    #[inline]
    pub fn toggle_channel(&mut self, channel: AudioChannel) {
        self.cpu.bus.apu.toggle_channel(channel);
    }

    /// Is control deck running.
    #[inline]
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }
}

impl ControlDeck {
    #[inline]
    fn clock_turbo(&mut self) {
        self.turbo_clock += 1;
        // Every 2 frames, ~30Hz turbo
        if self.turbo_clock > 2 {
            self.turbo_clock = 0;
        }
        let turbo = self.turbo_clock == 0;
        for gamepad in &mut self.cpu.bus.input.gamepads {
            if gamepad.turbo_a {
                gamepad.a = turbo;
            }
            if gamepad.turbo_b {
                gamepad.b = turbo;
            }
        }
    }
}

impl Default for ControlDeck {
    fn default() -> Self {
        Self::new(RamState::default())
    }
}

impl Clocked for ControlDeck {
    /// Steps the control deck a single clock cycle.
    #[inline]
    fn clock(&mut self) -> usize {
        self.cpu.clock()
    }
}

impl Powered for ControlDeck {
    /// Powers on the console
    #[inline]
    fn power_on(&mut self) {
        self.cpu.power_on();
        self.running = true;
    }

    /// Powers off the console
    #[inline]
    fn power_off(&mut self) {
        self.cpu.power_cycle();
        self.cpu.power_off();
        self.running = false;
    }

    /// Soft-resets the console
    #[inline]
    fn reset(&mut self) {
        self.cpu.reset();
        self.running = true;
    }

    /// Hard-resets the console
    #[inline]
    fn power_cycle(&mut self) {
        self.cpu.power_cycle();
        self.running = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemRead, MemWrite};
    use std::{fs::File, io::BufReader, path::PathBuf};

    fn load(file: &str) -> ControlDeck {
        let mut deck = ControlDeck::new(RamState::AllZeros);
        let rom = File::open(PathBuf::from(file)).unwrap();
        let mut rom = BufReader::new(rom);
        deck.load_rom(&file, &mut rom).unwrap();
        deck.power_on();
        deck
    }

    #[test]
    fn nestest() {
        let rom = "test_roms/cpu/nestest.nes";
        let mut deck = load(rom);
        deck.cpu.pc = 0xC000; // Start automated tests
        deck.clock_seconds(1.0);
        assert_eq!(deck.cpu.peek(0x0000), 0x00, "{}", rom);
    }

    #[test]
    fn dummy_writes_oam() {
        let rom = "test_roms/cpu/dummy_writes_oam.nes";
        let mut deck = load(rom);
        deck.clock_seconds(6.0);
        assert_eq!(deck.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn dummy_writes_ppumem() {
        let rom = "test_roms/cpu/dummy_writes_ppumem.nes";
        let mut deck = load(rom);
        deck.clock_seconds(4.0);
        assert_eq!(deck.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn exec_space_ppuio() {
        let rom = "test_roms/cpu/exec_space_ppuio.nes";
        let mut deck = load(rom);
        deck.clock_seconds(2.0);
        assert_eq!(deck.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn instr_timing() {
        let rom = "test_roms/cpu/instr_timing.nes";
        let mut deck = load(rom);
        deck.clock_seconds(23.0);
        assert_eq!(deck.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn apu_timing() {
        let rom = "test_roms/cpu/nestest.nes";
        let mut deck = load(rom);
        deck.cpu.bus.write(0x4017, 0x00);
        let mut irq_cycles = vec![];
        for _ in 0..=29840 {
            deck.clock();
            if deck.cpu.bus.apu.irq_pending {
                irq_cycles.push(deck.cpu.cycle_count);
                deck.cpu.bus.read(0x4015);
            }
        }
        assert_eq!(deck.cpu.cycle_count, 98172, "cpu cycle count should match");
        let frame_seq = deck.cpu.bus.apu.frame_sequencer();
        assert_eq!(
            frame_seq.divider.counter, 1626.5,
            "frame sequencer divider should match"
        );
        assert_eq!(
            frame_seq.sequencer.step, 2,
            "frame sequencer step should match"
        );
        assert_eq!(
            irq_cycles,
            vec![29831, 59662, 89491],
            "apu irq should occur on correct cycles"
        );
        assert!(!deck.cpu.bus.apu.irq_pending, "apu irq should be clear");
    }
}
