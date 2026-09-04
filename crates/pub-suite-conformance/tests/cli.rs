//! The binary as CI runs it, on synthetic callers, workflows and rules written to a scratch directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CI: &str = "name: ci
on:
  push: { branches: [main] }
  pull_request:
  merge_group:
jobs:
  suite:
    uses: acme/.github/.github/workflows/rust.yml@0123abc # v1.0.0
";
const RUST: &str = "name: rust
on:
  workflow_call:
jobs:
  probe:
    name: probe
  deny:
    name: deny
";
const RULES: &str = r#"{"rules": [{"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "suite / probe"}, {"context": "suite / deny"}]}}]}"#;

/// A scratch repository: `callers/ci.yml`, `workflows/rust.yml` and `rules.json`.
fn fixture(name: &str, rust_yml: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("conformance-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("callers")).unwrap();
    fs::create_dir_all(dir.join("workflows")).unwrap();
    fs::write(dir.join("callers/ci.yml"), CI).unwrap();
    fs::write(dir.join("workflows/rust.yml"), rust_yml).unwrap();
    fs::write(dir.join("rules.json"), RULES).unwrap();
    dir
}

fn conformance(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_conformance"))
        .arg("--callers")
        .arg(dir.join("callers"))
        .arg("--workflows")
        .arg(dir.join("workflows"))
        .arg("--rules")
        .arg(dir.join("rules.json"))
        .output()
        .expect("the binary runs")
}

#[test]
fn a_conforming_repository_exits_zero_and_lists_every_required_check() {
    let dir = fixture("clean", RUST);
    let out = conformance(&dir);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("required status checks: 2"), "{stdout}");
    assert!(stdout.contains("suite / deny"), "{stdout}");
    assert!(out.stderr.is_empty(), "{out:?}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_renamed_job_exits_one_and_names_the_missing_check_on_stderr() {
    let dir = fixture("renamed", &RUST.replace("name: deny", "name: deny-check"));
    let out = conformance(&dir);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing: suite / deny"), "{stderr}");
    assert!(stderr.contains("deny-check"), "{stderr}");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_wrong_command_line_exits_two_with_the_usage_on_stderr() {
    let out = Command::new(env!("CARGO_BIN_EXE_conformance"))
        .arg("--bogus")
        .output()
        .expect("the binary runs");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
}

#[test]
fn version_exits_zero_and_names_the_command() {
    let out = Command::new(env!("CARGO_BIN_EXE_conformance"))
        .arg("--version")
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("conformance "));
}
