//! End-to-end tests for the `patent` binary.
//!
//! These run the compiled binary as a subprocess and verify exit codes,
//! stderr diagnostics, and JSON output structure.

use std::process::Command;

fn patent_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_patent"));
    // Ensure the binary never tries to open a terminal (all tests pipe stdout).
    cmd.env("NO_COLOR", "1");
    cmd
}

// ── no-args behaviour ──────────────────────────────────────────────────

#[test]
fn no_args_piped_stdout_exits_with_error() {
    let out = patent_bin().output().expect("failed to run patent");
    assert!(!out.status.success(), "should fail when stdout is piped");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No idea provided") || stderr.contains("Usage"),
        "stderr should mention missing idea, got: {stderr}"
    );
}

// ── --json without idea ────────────────────────────────────────────────

#[test]
fn json_flag_without_idea_exits_with_error() {
    let out = patent_bin()
        .arg("--json")
        .output()
        .expect("failed to run patent");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--json requires an idea"),
        "should mention --json needs an idea, got: {stderr}"
    );
}

// ── --help always works ────────────────────────────────────────────────

#[test]
fn help_flag_exits_success() {
    let out = patent_bin()
        .arg("--help")
        .output()
        .expect("failed to run patent");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[IDEA]"),
        "idea should be shown as optional"
    );
    assert!(
        stdout.contains("interactive"),
        "should mention interactive search"
    );
}

// ── --version works ────────────────────────────────────────────────────

#[test]
fn version_flag_exits_success() {
    let out = patent_bin()
        .arg("--version")
        .output()
        .expect("failed to run patent");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("patent"),
        "version output should name the binary"
    );
}

// ── idea validation ────────────────────────────────────────────────────

#[test]
fn too_vague_idea_exits_with_error() {
    let out = patent_bin()
        .arg("hi")
        .output()
        .expect("failed to run patent");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Too vague") || stderr.contains("describe"),
        "should reject a one-word idea, got: {stderr}"
    );
}

#[test]
fn empty_idea_exits_with_error() {
    let out = patent_bin().arg("").output().expect("failed to run patent");
    assert!(!out.status.success());
}

// ── --json with a valid idea produces valid JSON ───────────────────────
// This test hits the network, so it's ignored by default. Run with:
//   cargo test --test cli -- --ignored

#[test]
#[ignore]
fn json_output_is_valid_json() {
    let out = patent_bin()
        .args([
            "--json",
            "--fast",
            "cli tool that kills a process on a given port",
        ])
        .output()
        .expect("failed to run patent");
    // Exit code reflects verdict level (0=Open, 1=Crowded, 2=Saturated),
    // so we only check that the process didn't crash (signal-killed).
    assert!(
        out.status.code().is_some(),
        "process should exit normally, not be killed by a signal"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert!(parsed.get("query").is_some(), "JSON should have 'query'");
    assert!(
        parsed.get("verdict").is_some(),
        "JSON should have 'verdict'"
    );
    assert!(
        parsed.get("matches").is_some(),
        "JSON should have 'matches'"
    );
}
