//! CLI query-path integration test (roadmap 1.2 V0-done criterion, D-12).
//! Covers `init` → `profile set`/`get`/`list` end to end against a real
//! temp world folder — not just the JSON Schema in isolation.

use assert_cmd::Command;
use tempfile::{tempdir, TempDir};

/// `init` best-effort registers the new world into the shared
/// `%APPDATA%/mapkeeper/projects.json` — override `APPDATA`/`HOME` to a throwaway
/// dir so running these tests never touches the real launcher's project list.
fn mapkeeper() -> Command {
    let mut cmd = Command::cargo_bin("mapkeeper").unwrap();
    cmd.env("APPDATA", fake_home().path()).env("HOME", fake_home().path());
    cmd
}

fn fake_home() -> &'static TempDir {
    use std::sync::OnceLock;
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| tempdir().unwrap())
}

#[test]
fn init_set_get_list_round_trip() {
    let dir = tempdir().unwrap();
    let world = dir.path();

    mapkeeper()
        .arg("init")
        .arg("ci-test")
        .arg("--path")
        .arg(world)
        .assert()
        .success();

    mapkeeper()
        .args(["profile", "set", "ci-test.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .args(["--title", "Old mill", "--notes", "Grinds grain"])
        .assert()
        .success();

    let get = mapkeeper()
        .args(["profile", "get", "ci-test.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(stdout.contains("\"display_name\": \"Old mill\""));
    assert!(stdout.contains("\"notes\": \"Grinds grain\""));

    let list = mapkeeper()
        .args(["profile", "list"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&list.get_output().stdout).to_string();
    assert!(stdout.contains("ci-test.hex.q0.r0"));
    assert!(stdout.contains("Old mill"));
}

#[test]
fn get_missing_profile_returns_blank_placeholder() {
    let dir = tempdir().unwrap();
    let world = dir.path();

    mapkeeper()
        .arg("init")
        .arg("ci-test2")
        .arg("--path")
        .arg(world)
        .assert()
        .success();

    let get = mapkeeper()
        .args(["profile", "get", "ci-test2.hex.q5.r5"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(stdout.contains("\"cell_id\": \"ci-test2.hex.q5.r5\""));
    assert!(stdout.contains("\"display_name\": \"\""));
}

#[test]
fn terrain_set_get_list_clear_round_trip() {
    let dir = tempdir().unwrap();
    let world = dir.path();

    mapkeeper()
        .arg("init")
        .arg("terra")
        .arg("--path")
        .arg(world)
        .assert()
        .success();

    // Unknown by default (nothing painted yet).
    let get = mapkeeper()
        .args(["terrain", "get", "terra.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(stdout.contains("\"state\": \"unknown\""));

    // Set a value.
    mapkeeper()
        .args(["terrain", "set", "terra.hex.q0.r0", "--value", "forest"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();

    // Set another cell explicitly to none.
    mapkeeper()
        .args(["terrain", "set", "terra.hex.q1.r0", "--none"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();

    let get = mapkeeper()
        .args(["terrain", "get", "terra.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(stdout.contains("\"state\": \"value\""));
    assert!(stdout.contains("\"value\": \"forest\""));

    let list = mapkeeper()
        .args(["terrain", "list"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&list.get_output().stdout).to_string();
    assert!(stdout.contains("terra.hex.q0.r0\tforest"));
    assert!(stdout.contains("terra.hex.q1.r0\tnone"));

    // Clear back to unknown.
    mapkeeper()
        .args(["terrain", "clear", "terra.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let get = mapkeeper()
        .args(["terrain", "get", "terra.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(stdout.contains("\"state\": \"unknown\""));
}

/// Terrain writes must never appear in profile JSON (D-36 boundary).
#[test]
fn terrain_edit_leaves_profile_untouched() {
    let dir = tempdir().unwrap();
    let world = dir.path();

    mapkeeper().arg("init").arg("sep").arg("--path").arg(world).assert().success();

    mapkeeper()
        .args(["profile", "set", "sep.hex.q0.r0", "--title", "Old mill"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    mapkeeper()
        .args(["terrain", "set", "sep.hex.q0.r0", "--value", "forest"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();

    let get = mapkeeper()
        .args(["profile", "get", "sep.hex.q0.r0"])
        .arg("--world")
        .arg(world)
        .assert()
        .success();
    let profile = String::from_utf8_lossy(&get.get_output().stdout).to_string();
    assert!(profile.contains("\"display_name\": \"Old mill\""));
    assert!(!profile.contains("forest"));
    assert!(!profile.contains("terrain"));
}

#[test]
fn get_rejects_malformed_cell_id() {
    let dir = tempdir().unwrap();
    let world = dir.path();

    mapkeeper()
        .arg("init")
        .arg("ci-test3")
        .arg("--path")
        .arg(world)
        .assert()
        .success();

    mapkeeper()
        .args(["profile", "get", "not-a-cell-id"])
        .arg("--world")
        .arg(world)
        .assert()
        .failure();
}
