//! Integration tests for `world_io` (N-031: tests outside implementation).

use super::*;

struct AppDataGuard {
    prev: Option<String>,
}

impl AppDataGuard {
    fn set(path: &Path) -> Self {
        let prev = std::env::var("APPDATA").ok();
        std::env::set_var("APPDATA", path);
        Self { prev }
    }
}

impl Drop for AppDataGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
    }
}

#[test]
fn path_key_normalizes_separators() {
    assert!(!path_cmp_key(Path::new("world")).contains('\\'));
}

#[test]
fn lexical_aliases_share_identity_key() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("world");
    fs::create_dir_all(&world).unwrap();
    let a = path_cmp_key(&world);
    let b = path_cmp_key(&dir.path().join("./world"));
    let c = path_cmp_key(&dir.path().join("x").join("..").join("world"));
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn upsert_registered_replaces_alias_written_entry() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("world");
    fs::create_dir_all(&world).unwrap();
    let alias = dir.path().join("x").join("..").join("world");

    let mut file = ProjectsFile::default();
    upsert_registered(
        &mut file,
        ProjectEntry {
            id: "first".into(),
            path: world.display().to_string(),
        },
    );
    upsert_registered(
        &mut file,
        ProjectEntry {
            id: "renamed".into(),
            path: alias.display().to_string(),
        },
    );

    assert_eq!(file.projects.len(), 1);
    assert_eq!(file.projects[0].id, "renamed");
    assert!(find_registered(&file, &world).is_some());
}

#[test]
fn create_fallback_key_uses_parent_canonical() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("nest");
    fs::create_dir_all(&parent).unwrap();
    let missing = parent.join("new-world");
    assert!(!missing.exists());
    let a = path_cmp_key(&missing);
    let b = path_cmp_key(&parent.join(".").join("new-world"));
    assert_eq!(a, b);
    assert!(a.contains("new-world") || a.ends_with("new-world"));
}

#[test]
fn symlink_or_junction_alias_shares_key_when_supported() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real-world");
    fs::create_dir_all(&real).unwrap();
    let alias = dir.path().join("alias-world");
    let linked = create_dir_alias(&real, &alias);
    if !linked {
        return;
    }
    assert_eq!(path_cmp_key(&real), path_cmp_key(&alias));
}

fn create_dir_alias(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        // Junction does not require elevated privileges.
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.display().to_string(),
                &target.display().to_string(),
            ])
            .status();
        status.map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (target, link);
        false
    }
}

#[test]
fn missing_registry_is_empty_not_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.json");
    let file = load_projects_from(&path).unwrap();
    assert!(file.projects.is_empty());
}

#[test]
fn malformed_registry_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    fs::write(&path, "{broken").unwrap();
    let err = load_projects_from(&path).unwrap_err();
    assert!(err.starts_with("corrupt_registry:"));
}

#[test]
fn missing_registry_with_valid_bak_is_recovery_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let bak = crate::atomic_io::bak_path(&path);
    let good = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "w".into(),
            path: "/world".into(),
        }],
    };
    fs::write(&bak, good.to_json_pretty().unwrap()).unwrap();
    let err = load_projects_from(&path).unwrap_err();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=true"));
    assert!(!path.is_file());
    assert!(bak.is_file());
}

#[test]
fn restore_reinstates_bak_and_quarantines_corrupt_primary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let good = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "kept".into(),
            path: "/world".into(),
        }],
    };
    fs::write(
        crate::atomic_io::bak_path(&path),
        good.to_json_pretty().unwrap(),
    )
    .unwrap();
    fs::write(&path, "{broken").unwrap();

    let restored = restore_projects_from_bak_at(&path).unwrap();
    assert_eq!(restored.projects[0].id, "kept");
    assert_eq!(load_projects_from(&path).unwrap().projects[0].id, "kept");
    let quarantined: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("projects.json.corrupt-")
        })
        .collect();
    assert_eq!(
        quarantined.len(),
        1,
        "corrupt primary is kept for diagnosis"
    );
    assert_eq!(
        fs::read_to_string(quarantined[0].path()).unwrap(),
        "{broken"
    );
}

#[test]
fn restore_recovers_interrupted_write_without_primary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let good = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "kept".into(),
            path: "/world".into(),
        }],
    };
    fs::write(
        crate::atomic_io::bak_path(&path),
        good.to_json_pretty().unwrap(),
    )
    .unwrap();
    assert_eq!(
        restore_projects_from_bak_at(&path).unwrap().projects[0].id,
        "kept"
    );
    assert!(path.is_file());
}

#[test]
fn restore_refuses_when_bak_is_absent_or_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    fs::write(&path, "{broken").unwrap();
    assert!(restore_projects_from_bak_at(&path)
        .unwrap_err()
        .contains("no bak available"));

    fs::write(crate::atomic_io::bak_path(&path), "{also-broken").unwrap();
    assert!(restore_projects_from_bak_at(&path)
        .unwrap_err()
        .contains("invalid bak"));
    // Corrupt primary stays untouched when recovery refuses.
    assert_eq!(fs::read_to_string(&path).unwrap(), "{broken");
}

#[test]
fn missing_registry_with_invalid_bak_never_empty_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let bak = crate::atomic_io::bak_path(&path);
    fs::write(&bak, "{broken").unwrap();
    let err = load_projects_from(&path).unwrap_err();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=false"));
    assert!(!path.is_file());
}

#[test]
fn registry_failpoint_after_bak_survives_restart() {
    crate::atomic_io::clear_failpoint();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let old = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "old".into(),
            path: "/old".into(),
        }],
    };
    save_projects_to(&path, &old).unwrap();
    let next = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "new".into(),
            path: "/new".into(),
        }],
    };
    crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::AfterPrimaryToBak);
    assert!(save_projects_to(&path, &next).is_err());
    assert!(!path.is_file());
    assert!(crate::atomic_io::bak_path(&path).is_file());
    let err = load_projects_from(&path).unwrap_err();
    assert!(err.contains("interrupted_write"));
    assert!(err.contains("bak_available=true"));
}

#[test]
fn registry_failpoint_final_rename_restores_primary() {
    crate::atomic_io::clear_failpoint();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let old = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "old".into(),
            path: "/old".into(),
        }],
    };
    save_projects_to(&path, &old).unwrap();
    let next = ProjectsFile {
        projects: vec![mapkeeper_core::projects::ProjectEntry {
            id: "new".into(),
            path: "/new".into(),
        }],
    };
    crate::atomic_io::set_failpoint(crate::atomic_io::AtomicFailAt::FinalRename);
    assert!(save_projects_to(&path, &next).is_err());
    let loaded = load_projects_from(&path).unwrap();
    assert_eq!(loaded.projects[0].id, "old");
}

#[test]
fn cleanup_refuses_without_marker() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("user-notes");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("notes.txt"), "keep").unwrap();
    let registry = ProjectsFile::default();
    assert!(cleanup_incomplete_create(&world, &registry).is_err());
    assert!(world.join("notes.txt").is_file());
}

#[test]
fn cleanup_removes_marked_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("partial");
    fs::create_dir_all(&world).unwrap();
    write_incomplete_marker(&world).unwrap();
    fs::write(world.join("mapkeeper.toml"), "x").unwrap();
    let registry = ProjectsFile::default();
    cleanup_incomplete_create(&world, &registry).unwrap();
    assert!(!world.exists());
}

#[test]
fn classify_pure_states() {
    assert_eq!(
        classify_create_marker(&CreateDiskFacts {
            has_marker: false,
            valid_manifest_id: None,
            valid_spatial: false,
            has_foreign_entries: false,
            registry_id: None,
        }),
        CreateMarkerClass::NoMarker
    );
    assert_eq!(
        classify_create_marker(&CreateDiskFacts {
            has_marker: true,
            valid_manifest_id: None,
            valid_spatial: false,
            has_foreign_entries: false,
            registry_id: None,
        }),
        CreateMarkerClass::SafeIncomplete
    );
    assert_eq!(
        classify_create_marker(&CreateDiskFacts {
            has_marker: true,
            valid_manifest_id: Some("w".into()),
            valid_spatial: true,
            has_foreign_entries: false,
            registry_id: Some("w".into()),
        }),
        CreateMarkerClass::CompleteRegistered {
            world_id: "w".into()
        }
    );
    assert_eq!(
        classify_create_marker(&CreateDiskFacts {
            has_marker: true,
            valid_manifest_id: Some("w".into()),
            valid_spatial: true,
            has_foreign_entries: false,
            registry_id: None,
        }),
        CreateMarkerClass::CompleteUnregistered {
            world_id: "w".into()
        }
    );
    assert_eq!(
        classify_create_marker(&CreateDiskFacts {
            has_marker: true,
            valid_manifest_id: Some("w".into()),
            valid_spatial: false,
            has_foreign_entries: true,
            registry_id: None,
        }),
        CreateMarkerClass::Ambiguous {
            reason: "foreign_entries"
        }
    );
}

#[test]
fn planted_marker_with_author_files_refuses_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let world = dir.path().join("user-folder");
    fs::create_dir_all(&world).unwrap();
    write_incomplete_marker(&world).unwrap();
    fs::write(world.join("notes.txt"), "author lore").unwrap();
    let registry = ProjectsFile::default();
    let err = cleanup_incomplete_create(&world, &registry).unwrap_err();
    assert!(err.contains("refuse cleanup"));
    assert!(world.join("notes.txt").is_file());
    assert!(is_incomplete_create(&world));
}

#[test]
fn corrupt_registry_refuses_create_cleanup() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::var("APPDATA").ok();
    std::env::set_var("APPDATA", dir.path());
    let world = dir.path().join("worlds").join("partial");
    fs::create_dir_all(&world).unwrap();
    write_incomplete_marker(&world).unwrap();
    fs::write(world.join("scratch.tmp"), "x").unwrap();
    let path = projects_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, "{broken").unwrap();
    let err = cleanup_after_failed_create_checked(&world).unwrap_err();
    assert!(err.contains("refuse cleanup"));
    assert!(err.contains("corrupt_registry") || err.contains("unreadable registry"));
    assert!(world.exists());
    assert!(world.join(CREATE_INCOMPLETE_MARKER).is_file());
    match prev {
        Some(v) => std::env::set_var("APPDATA", v),
        None => std::env::remove_var("APPDATA"),
    }
}

#[test]
fn interrupted_registry_refuses_create_cleanup() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::var("APPDATA").ok();
    std::env::set_var("APPDATA", dir.path());
    let world = dir.path().join("worlds").join("partial2");
    fs::create_dir_all(&world).unwrap();
    write_incomplete_marker(&world).unwrap();
    let path = projects_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    // Interrupted write: primary absent, bak present.
    fs::write(crate::atomic_io::bak_path(&path), r#"{"projects":[]}"#).unwrap();
    let err = cleanup_after_failed_create_checked(&world).unwrap_err();
    assert!(err.contains("refuse cleanup"));
    assert!(err.contains("interrupted_write") || err.contains("unreadable registry"));
    assert!(world.exists());
    match prev {
        Some(v) => std::env::set_var("APPDATA", v),
        None => std::env::remove_var("APPDATA"),
    }
}

#[test]
fn malformed_delete_inflight_is_observable() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::var("APPDATA").ok();
    std::env::set_var("APPDATA", dir.path());
    let root = delete_inflight_root();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("bad.json"), "{not-json").unwrap();
    let list = list_delete_inflights().unwrap();
    assert!(list.records.is_empty());
    assert!(!list.diagnostics.is_empty());
    let err = reconcile_delete_inflights().unwrap_err();
    assert!(err.contains("malformed") || err.contains("inflight"));
    assert!(root.join("bad.json").is_file());
    match prev {
        Some(v) => std::env::set_var("APPDATA", v),
        None => std::env::remove_var("APPDATA"),
    }
}

#[test]
fn complete_registered_with_marker_refuses_cleanup() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let prev = std::env::var("APPDATA").ok();
    std::env::set_var("APPDATA", dir.path());
    let world = dir.path().join("worlds").join("done");
    let map_path = crate::world_layout::write_world_skeleton(
        &world,
        "done",
        mapkeeper_core::spatial::alpha_default_preset(),
    )
    .unwrap();
    let state = mapkeeper_core::spatial::default_spatial_state();
    let mut state = state;
    state.revision = 1;
    fs::create_dir_all(map_path.join("spatial")).unwrap();
    fs::write(
        map_path.join("spatial/state.json"),
        state.to_json_pretty().unwrap(),
    )
    .unwrap();
    write_incomplete_marker(&world).unwrap();
    let mut file = ProjectsFile::default();
    upsert_registered(
        &mut file,
        mapkeeper_core::projects::ProjectEntry {
            id: "done".into(),
            path: world.display().to_string(),
        },
    );
    save_projects(&file).unwrap();
    assert!(matches!(
        classify_create_marker_at(&world, &file),
        CreateMarkerClass::CompleteRegistered { .. }
    ));
    assert!(cleanup_incomplete_create(&world, &file).is_err());
    assert!(world.join("mapkeeper.toml").is_file());
    match prev {
        Some(v) => std::env::set_var("APPDATA", v),
        None => std::env::remove_var("APPDATA"),
    }
}

#[test]
fn trash_collision_safe_names() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let _guard = AppDataGuard::set(dir.path());
    let a = allocate_trash_dir("wid").unwrap();
    fs::create_dir_all(&a).unwrap();
    let b = allocate_trash_dir("wid").unwrap();
    assert_ne!(a, b);
}

#[test]
fn move_to_trash_leaves_origin_note() {
    let _lock = lock_appdata_env();
    let dir = tempfile::tempdir().unwrap();
    let _guard = AppDataGuard::set(dir.path());
    let world = dir.path().join("worlds").join("w1");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("mapkeeper.toml"), "ok").unwrap();
    let trash = move_world_to_trash(&world, "w1").unwrap();
    assert!(!world.exists());
    assert!(trash.join("mapkeeper.toml").is_file());
    assert!(trash.join("mapkeeper-trash-origin.txt").is_file());
}
