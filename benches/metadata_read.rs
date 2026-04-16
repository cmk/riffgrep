//! Criterion benchmarks for metadata reading.

use std::io::Cursor;
use std::path::Path;

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use riffgrep::engine::bext;

fn bench_scan_chunks(c: &mut Criterion) {
    let path = Path::new("test_files/all_riff_info_tags_with_numbers.wav");
    if !path.exists() {
        eprintln!("skipping bench: test_files not found");
        return;
    }
    let data = std::fs::read(path).unwrap();

    c.bench_function("scan_chunks/all_riff_info", |b| {
        b.iter(|| {
            let mut cursor = Cursor::new(black_box(&data));
            bext::scan_chunks(&mut cursor).unwrap()
        })
    });
}

fn bench_parse_bext_buffer(c: &mut Criterion) {
    let buf = [0u8; 602];
    c.bench_function("parse_bext_buffer/zeros", |b| {
        b.iter(|| bext::parse_bext_buffer(black_box(&buf)))
    });
}

fn bench_read_metadata(c: &mut Criterion) {
    let files = [
        "test_files/all_riff_info_tags_with_numbers.wav",
        "test_files/clean_base.wav",
        "test_files/id3-all_sm.wav",
    ];

    for path_str in &files {
        let path = Path::new(path_str);
        if !path.exists() {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        c.bench_function(&format!("read_metadata/{name}"), |b| {
            b.iter(|| riffgrep::engine::read_metadata(black_box(path)).unwrap())
        });
    }
}

fn bench_search_query_matches(c: &mut Criterion) {
    use riffgrep::engine::{MatchMode, Pattern, SearchQuery, UnifiedMetadata};

    let meta = UnifiedMetadata {
        vendor: "Samples From Mars".to_string(),
        library: "DX100 From Mars".to_string(),
        category: "LOOP".to_string(),
        description: "A cool loop".to_string(),
        ..Default::default()
    };

    let query = SearchQuery {
        vendor: Some(Pattern::Substring("mars".to_string())),
        category: Some(Pattern::Substring("loop".to_string())),
        match_mode: MatchMode::And,
        ..Default::default()
    };

    c.bench_function("SearchQuery::matches/and_2_fields", |b| {
        b.iter(|| query.matches(black_box(&meta)))
    });
}

fn bench_filesystem_vs_sqlite(c: &mut Criterion) {
    use riffgrep::engine::filesystem::FilesystemFinder;
    use riffgrep::engine::sqlite::Database;
    use riffgrep::engine::{Pattern, SearchQuery, UnifiedMetadata};
    use std::path::PathBuf;

    let test_dir = PathBuf::from("test_files");
    if !test_dir.exists() {
        eprintln!("skipping bench: test_files not found");
        return;
    }

    // Pre-populate an in-memory DB with test files.
    let db = Database::open_in_memory().unwrap();
    for entry in std::fs::read_dir(&test_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "wav") {
            if let Ok(meta) = riffgrep::engine::read_metadata(&path) {
                db.insert_batch(&[(meta, 0, None)]).unwrap();
            }
        }
    }

    let query = SearchQuery {
        vendor: Some(Pattern::Substring("IART".to_string())),
        ..Default::default()
    };

    c.bench_function("search/filesystem_9_files", |b| {
        b.iter(|| {
            let finder = FilesystemFinder::new(vec![test_dir.clone()], 1);
            let (tx, rx) = crossbeam_channel::bounded(64);
            let q = query.clone();
            std::thread::spawn(move || finder.walk(&q, tx));
            let _: Vec<UnifiedMetadata> = rx.iter().collect();
        })
    });

    c.bench_function("search/sqlite_9_files", |b| {
        b.iter(|| {
            let (tx, rx) = crossbeam_channel::bounded(64);
            db.search(black_box(&query), &tx);
            drop(tx);
            let _: Vec<UnifiedMetadata> = rx.iter().collect();
        })
    });
}

criterion_group!(
    benches,
    bench_scan_chunks,
    bench_parse_bext_buffer,
    bench_read_metadata,
    bench_search_query_matches,
    bench_filesystem_vs_sqlite,
);
criterion_main!(benches);
