//! Edge case integration tests.
//!
//! These test the binary with unusual inputs.

use assert_cmd::Command;
use predicates::prelude::*;

fn riffgrep() -> Command {
    let mut cmd = Command::cargo_bin("riffgrep").unwrap();
    cmd.arg("--no-db"); // Force filesystem mode so tests aren't affected by an existing index.
    cmd
}

#[test]
fn search_id3_only_file() {
    // id3-only.wav has no bext chunk in the header, just fmt+data+id3+LIST-INFO.
    // LIST-INFO is after audio data (>4KB) so won't be found by fast scan.
    // File should still appear with no filters.
    riffgrep()
        .args(["./test_files/id3-only.wav"])
        .assert()
        .success()
        .stdout(predicate::str::contains("id3-only.wav"));
}

#[test]
fn search_id3_r7_file() {
    // id3-all_r7.wav has JUNK at offset 36, bext after audio (>4KB).
    // Should still be found (it's a valid WAV).
    riffgrep()
        .args(["./test_files/id3-all_r7.wav"])
        .assert()
        .success()
        .stdout(predicate::str::contains("id3-all_r7.wav"));
}

#[test]
fn verbose_on_clean_base() {
    // clean_base.wav has a BEXT description but no originator/vendor.
    riffgrep()
        .args(["--verbose", "./test_files/clean_base.wav"])
        .assert()
        .success()
        .stdout(predicate::str::contains("description: Yamaha DX-100"));
}

#[test]
fn json_on_sm_file() {
    riffgrep()
        .args(["--json", "./test_files/clean_base.wav"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"description\":\"Yamaha DX-100\""));
}

#[test]
fn or_mode() {
    // Search for vendor="IART-Artist" OR description="Yamaha"
    // Should match both all_riff_info_tags_with_numbers.wav and clean_base.wav.
    let output = riffgrep()
        .args([
            "--or",
            "--vendor", "IART-Artist",
            "--description", "Yamaha",
            "./test_files/",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("all_riff_info_tags_with_numbers.wav"));
    assert!(stdout.contains("clean_base.wav"));
}
