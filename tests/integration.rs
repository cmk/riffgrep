//! End-to-end integration tests using assert_cmd.

use assert_cmd::Command;
use predicates::prelude::*;

fn riffgrep() -> Command {
    Command::cargo_bin("riffgrep").unwrap()
}

#[test]
fn search_by_category() {
    riffgrep()
        .args(["--category", "IGNR-Genre", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "all_riff_info_tags_with_numbers.wav",
        ));
}

#[test]
fn no_matches_exit_code_1() {
    riffgrep()
        .args(["--vendor", "nonexistent_vendor_xyz", "./test_files/"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty());
}

#[test]
fn count_all_files() {
    riffgrep()
        .args(["--count", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));
}

#[test]
fn json_output_valid() {
    let output = riffgrep()
        .args(["--json", "--category", "IGNR-Genre", "./test_files/"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let line = String::from_utf8(output).unwrap();
    let _: serde_json::Value = serde_json::from_str(line.trim()).expect("valid JSON");
}

#[test]
fn verbose_output() {
    riffgrep()
        .args(["--verbose", "--category", "IGNR-Genre", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("vendor: IART-Artist"))
        .stdout(predicate::str::contains("category: IGNR-Genre"));
}

#[test]
fn no_filters_lists_all_wav() {
    let output = riffgrep()
        .args(["./test_files/"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines: Vec<&str> = String::from_utf8(output.clone())
        .unwrap()
        .leak()
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines.len(), 9, "expected 9 WAV files");
}

#[test]
fn help_exit_code_0() {
    riffgrep()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Available options"));
}

#[test]
fn nonexistent_path_stderr() {
    riffgrep()
        .arg("/nonexistent/directory/abc123")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("No such file or directory"));
}

#[test]
fn description_filter() {
    riffgrep()
        .args(["--description", "Yamaha", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("clean_base.wav"));
}

#[test]
fn regex_filter() {
    riffgrep()
        .args(["--regex", "--description", "DX-?1[0-9]{2}", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("clean_base.wav"));
}

// --- SQLite E2E tests ---

#[test]
fn index_test_files() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_index.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Indexed 9 files"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn index_idempotent() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_idempotent.db");
    let _ = std::fs::remove_file(&db_path);

    // Index twice.
    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    // Second index should find 0 new files (all mtimes match).
    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Indexed 0 files"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sqlite_search_after_index() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_search.db");
    let _ = std::fs::remove_file(&db_path);

    // Index.
    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    // Search using SQLite mode.
    riffgrep()
        .args([
            "--vendor",
            "IART",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("all_riff_info_tags_with_numbers"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sqlite_count_mode() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_count.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    riffgrep()
        .args([
            "--count",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn no_db_overrides_sqlite() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_nodb.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    // --no-db forces filesystem mode even though DB exists.
    riffgrep()
        .args([
            "--no-db",
            "--count",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn db_stats_after_index() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_stats.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    riffgrep()
        .args([
            "--db-stats",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Files:    9"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn db_stats_no_db_error() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_nostats.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--db-stats",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("database not found"));
}

#[test]
fn force_reindex() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_force.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    // Force re-index should re-parse all files.
    riffgrep()
        .args([
            "--index",
            "--force-reindex",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Indexed 9 files"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sqlite_verbose_output() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_verbose.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    riffgrep()
        .args([
            "--verbose",
            "--vendor",
            "IART",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("vendor: IART-Artist"))
        .stdout(predicate::str::contains("category: IGNR-Genre"));

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn sqlite_json_output() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_json.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "./test_files/",
        ])
        .assert()
        .success();

    let output = riffgrep()
        .args([
            "--json",
            "--vendor",
            "IART",
            "--db-path",
            db_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let line = String::from_utf8(output).unwrap();
    let _: serde_json::Value = serde_json::from_str(line.trim()).expect("valid JSON");

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn index_nonexistent_path() {
    let db_path = std::env::temp_dir().join("riffgrep_e2e_nonexistent.db");
    let _ = std::fs::remove_file(&db_path);

    riffgrep()
        .args([
            "--index",
            "--db-path",
            db_path.to_str().unwrap(),
            "/nonexistent/directory/abc123",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Indexed 0 files"));

    let _ = std::fs::remove_file(&db_path);
}

// --- Sprint 3: TUI-related E2E tests ---

#[test]
fn headless_e2e_still_works() {
    // --no-tui with search filters should produce same headless output as before.
    riffgrep()
        .args(["--no-tui", "--vendor", "IART", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("all_riff_info_tags_with_numbers"));
}

#[test]
fn piped_output_headless() {
    // Piped output (non-TTY) should remain headless.
    // assert_cmd captures stdout, so it's not a TTY.
    riffgrep()
        .args(["--count", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));
}

#[test]
fn no_tui_forces_headless() {
    // --no-tui prevents TUI even without search filters.
    riffgrep()
        .args(["--no-tui", "--count", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));
}

#[test]
fn theme_flag_parsed() {
    // --theme is accepted without error (headless mode with filter).
    riffgrep()
        .args(["--no-tui", "--theme", "ableton", "--count", "./test_files/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("9 matches"));
}

#[test]
fn no_tui_flag_parsed() {
    // --no-tui flag is accepted.
    riffgrep()
        .args(["--no-tui", "--vendor", "nonexistent_xyz", "./test_files/"])
        .assert()
        .code(1);
}
