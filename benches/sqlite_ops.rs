//! Criterion benchmarks for SQLite operations.

use std::path::PathBuf;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use riffgrep::engine::sqlite::{self, Database};
use riffgrep::engine::{
    BpmRange, MatchMode, Pattern, SearchQuery, UnifiedMetadata,
};

fn make_test_meta(i: usize) -> UnifiedMetadata {
    UnifiedMetadata {
        path: PathBuf::from(format!("/samples/vendor_{}/lib_{}/file_{i}.wav", i % 50, i % 200)),
        vendor: format!("Vendor {}", i % 50),
        library: format!("Library {}", i % 200),
        category: format!("Category {}", i % 10),
        sound_id: format!("SID{i:06}"),
        description: format!("This is a sample description for file number {i}"),
        comment: format!("Comment {i}"),
        key: if i % 12 == 0 { "C".to_string() } else { "".to_string() },
        bpm: if i % 3 == 0 { Some((80 + (i % 100)) as u16) } else { None },
        rating: if i % 4 == 0 { "****".to_string() } else { "".to_string() },
        subcategory: "".to_string(),
        genre_id: "".to_string(),
        usage_id: "".to_string(),
        umid: "".to_string(),
        recid: 0,
        take: "".to_string(),
        track: "".to_string(),
        item: "".to_string(),
        date: "".to_string(),
    }
}

fn populate_db(db: &Database, count: usize) {
    let records: Vec<(UnifiedMetadata, i64, Option<Vec<u8>>)> = (0..count)
        .map(|i| (make_test_meta(i), (1700000000 + i as i64), None))
        .collect();

    for chunk in records.chunks(1000) {
        db.insert_batch(chunk).unwrap();
    }
}

fn bench_fts5_trigram_query(c: &mut Criterion) {
    let db = Database::open_in_memory().unwrap();
    populate_db(&db, 10_000);

    c.bench_function("fts5_trigram_query/10k_rows", |b| {
        b.iter(|| {
            let query = SearchQuery {
                vendor: Some(Pattern::Substring("Vendor 25".to_string())),
                ..Default::default()
            };
            let (tx, rx) = crossbeam_channel::bounded(1024);
            db.search(black_box(&query), &tx);
            drop(tx);
            let _: Vec<_> = rx.iter().collect();
        })
    });
}

fn bench_batch_insert_1000(c: &mut Criterion) {
    c.bench_function("batch_insert/1000_rows", |b| {
        b.iter(|| {
            let db = Database::open_in_memory().unwrap();
            let records: Vec<(UnifiedMetadata, i64, Option<Vec<u8>>)> = (0..1000)
                .map(|i| (make_test_meta(i), 100, None))
                .collect();
            db.insert_batch(black_box(&records)).unwrap()
        })
    });
}

fn bench_query_builder(c: &mut Criterion) {
    let query = SearchQuery {
        vendor: Some(Pattern::Substring("mars".to_string())),
        category: Some(Pattern::Substring("loop".to_string())),
        bpm: Some(BpmRange { min: 120, max: 128 }),
        match_mode: MatchMode::And,
        ..Default::default()
    };

    c.bench_function("build_sql/3_fields", |b| {
        b.iter(|| sqlite::build_sql(black_box(&query)))
    });
}

fn bench_peak_compress(c: &mut Criterion) {
    let raw: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();

    c.bench_function("peak_compress/180_bytes", |b| {
        b.iter(|| sqlite::compress_peaks(black_box(&raw)))
    });
}

fn bench_peak_decompress(c: &mut Criterion) {
    let raw: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();
    let compressed = sqlite::compress_peaks(&raw);

    c.bench_function("peak_decompress/180_bytes", |b| {
        b.iter(|| sqlite::decompress_peaks(black_box(&compressed)))
    });
}

fn bench_like_query_10k(c: &mut Criterion) {
    let db = Database::open_in_memory().unwrap();
    populate_db(&db, 10_000);

    c.bench_function("like_query/2_fields_10k", |b| {
        b.iter(|| {
            let query = SearchQuery {
                vendor: Some(Pattern::Substring("Vendor 25".to_string())),
                category: Some(Pattern::Substring("Category 3".to_string())),
                match_mode: MatchMode::And,
                ..Default::default()
            };
            let (tx, rx) = crossbeam_channel::bounded(1024);
            db.search(black_box(&query), &tx);
            drop(tx);
            let _: Vec<_> = rx.iter().collect();
        })
    });
}

fn bench_empty_query_10k(c: &mut Criterion) {
    let db = Database::open_in_memory().unwrap();
    populate_db(&db, 10_000);

    c.bench_function("empty_query/10k_rows", |b| {
        b.iter(|| {
            let query = SearchQuery::default();
            let (tx, rx) = crossbeam_channel::bounded(16384);
            db.search(black_box(&query), &tx);
            drop(tx);
            let _: Vec<_> = rx.iter().collect();
        })
    });
}

criterion_group!(
    benches,
    bench_fts5_trigram_query,
    bench_batch_insert_1000,
    bench_query_builder,
    bench_peak_compress,
    bench_peak_decompress,
    bench_like_query_10k,
    bench_empty_query_10k,
);
criterion_main!(benches);
