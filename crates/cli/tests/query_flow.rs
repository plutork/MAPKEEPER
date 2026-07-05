//! CLI query-path integration test (roadmap 1.2 V0-done criterion, D-12).
//! Covers `init` → `profile set`/`get`/`list` end to end against a real
//! temp world folder — not just the JSON Schema in isolation.

use assert_cmd::Command;
use tempfile::tempdir;

fn mapkeeper() -> Command {
    Command::cargo_bin("mapkeeper").unwrap()
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
