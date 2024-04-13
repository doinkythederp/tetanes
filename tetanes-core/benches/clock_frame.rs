use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::{fs::File, io::BufReader, path::Path, time::Duration};
use tetanes_core::{
    control_deck::{Config, ControlDeck},
    mem::RamState,
};

fn clock_frames(rom_path: impl AsRef<Path>, frames: u32) {
    let rom_path = rom_path.as_ref();
    assert!(rom_path.exists(), "No test rom found for {rom_path:?}");
    let mut rom = BufReader::new(File::open(rom_path).expect("failed to open path"));
    let mut deck = ControlDeck::with_config(Config {
        ram_state: RamState::AllZeros,
        ..Default::default()
    });
    deck.load_rom(&rom_path.to_string_lossy(), &mut rom)
        .expect("failed to load rom");
    while deck.frame_number() < frames {
        deck.clock_frame().expect("valid frame clock");
        deck.clear_audio_samples();
    }
}

fn basic(c: &mut Criterion) {
    let mut group = c.benchmark_group("nes");
    group.measurement_time(Duration::from_secs(10));
    group.bench_function("basic", |b| {
        b.iter(|| clock_frames("../test_roms/ppu/_240pee.nes", black_box(200)))
    });
    group.finish();
}

fn stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("nes");
    group.measurement_time(Duration::from_secs(60));
    group.sample_size(10);
    group.bench_function("stress", |b| {
        b.iter(|| clock_frames("../test_roms/spritecans.nes", black_box(1000)));
    });
    group.finish();
}

criterion_group!(benches, basic, stress);
criterion_main!(benches);
