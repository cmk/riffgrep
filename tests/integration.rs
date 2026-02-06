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
