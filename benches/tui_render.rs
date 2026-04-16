//! Criterion benchmarks for TUI rendering operations.
#![allow(missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use riffgrep::engine::sqlite;
use riffgrep::ui::widgets::{render_braille_waveform, render_braille_waveform_height};

fn bench_braille_render_16row(c: &mut Criterion) {
    let peaks: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();
    c.bench_function("braille_render_16row_w90", |b| {
        b.iter(|| render_braille_waveform(black_box(&peaks), black_box(90)))
    });
}

fn bench_braille_render_4row(c: &mut Criterion) {
    let peaks: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();
    c.bench_function("braille_render_4row_w90", |b| {
        b.iter(|| render_braille_waveform_height(black_box(&peaks), black_box(90), black_box(4)))
    });
}

fn bench_braille_render_resample_wide(c: &mut Criterion) {
    let peaks: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();
    c.bench_function("braille_render_16row_w200", |b| {
        b.iter(|| render_braille_waveform(black_box(&peaks), black_box(200)))
    });
}

fn bench_peak_decompress_and_render(c: &mut Criterion) {
    let raw: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
    let compressed = sqlite::compress_peaks(&raw);

    c.bench_function("peak_decompress_and_render_16row", |b| {
        b.iter(|| {
            let decompressed = sqlite::decompress_peaks(black_box(&compressed));
            render_braille_waveform(black_box(&decompressed), black_box(90))
        })
    });
}

criterion_group!(
    benches,
    bench_braille_render_16row,
    bench_braille_render_4row,
    bench_braille_render_resample_wide,
    bench_peak_decompress_and_render,
);
criterion_main!(benches);
