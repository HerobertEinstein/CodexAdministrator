#![cfg(windows)]

use std::{
    fs,
    os::windows::fs::{symlink_dir, symlink_file},
};

use codex_administrator::sync_native_skills;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[test]
fn custom_skills_are_projected_without_touching_official_system_skills() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    fs::create_dir_all(daily.join("skills/custom-skill")).unwrap();
    fs::create_dir_all(daily.join("skills/.system")).unwrap();
    fs::create_dir_all(isolated.join("skills/.system")).unwrap();
    fs::write(
        daily.join("skills/custom-skill/SKILL.md"),
        "# Custom skill\n",
    )
    .unwrap();
    fs::write(
        daily.join("skills/.system/SKILL.md"),
        "# Daily official system skill\n",
    )
    .unwrap();
    fs::write(
        isolated.join("skills/.system/SKILL.md"),
        "# Isolated official system skill\n",
    )
    .unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 1);
    assert_eq!(receipt.conflicts, 0);
    assert_eq!(
        fs::read_to_string(isolated.join("skills/custom-skill/SKILL.md")).unwrap(),
        "# Custom skill\n"
    );
    assert_eq!(
        fs::read_to_string(isolated.join("skills/.system/SKILL.md")).unwrap(),
        "# Isolated official system skill\n"
    );
}

#[test]
fn unchanged_projection_follows_source_updates_and_records_both_hashes() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Version one\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();

    fs::write(&source, "# Version two\n").unwrap();
    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.updated, 1);
    assert_eq!(receipt.conflicts, 0);
    assert_eq!(
        fs::read_to_string(isolated.join("skills/custom-skill/SKILL.md")).unwrap(),
        "# Version two\n"
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    let record = &manifest["files"]["custom-skill/SKILL.md"];
    assert_eq!(record["source_sha256"], record["destination_sha256"]);
    assert_eq!(record["source_sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn unchanged_projection_is_removed_after_the_daily_source_is_deleted() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Removable\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();

    fs::remove_file(&source).unwrap();
    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.removed, 1);
    assert_eq!(receipt.conflicts, 0);
    assert!(!isolated.join("skills/custom-skill/SKILL.md").exists());
    assert!(!isolated.join("skills/custom-skill").exists());
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    assert!(manifest["files"].as_object().unwrap().is_empty());
}

#[test]
fn locally_modified_projection_is_preserved_and_recorded_for_review() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Version one\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();

    fs::write(&destination, "# Isolated local edit\n").unwrap();
    fs::write(&source, "# Version two\n").unwrap();
    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.updated, 0);
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        "# Isolated local edit\n"
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    let conflict = &manifest["conflicts"]["custom-skill/SKILL.md"];
    assert_eq!(conflict["reason"], "destination_modified");
    assert_eq!(conflict["source_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(conflict["destination_sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn locally_deleted_projection_is_not_recreated_automatically() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Keep source\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();

    fs::remove_file(&destination).unwrap();
    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 0);
    assert_eq!(receipt.conflicts, 1);
    assert!(!destination.exists());
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    let conflict = &manifest["conflicts"]["custom-skill/SKILL.md"];
    assert_eq!(conflict["reason"], "destination_missing");
    assert!(conflict["destination_sha256"].is_null());
}

#[test]
fn preexisting_unmanaged_isolated_skill_is_never_claimed_or_overwritten() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&source, "# Daily canonical\n").unwrap();
    fs::write(&destination, "# Isolated unmanaged\n").unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 0);
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        "# Isolated unmanaged\n"
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    assert!(manifest["files"].as_object().unwrap().is_empty());
    assert_eq!(
        manifest["conflicts"]["custom-skill/SKILL.md"]["reason"],
        "unmanaged_destination"
    );
}

#[test]
fn cache_and_temporary_residue_are_excluded_from_the_projection() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let skill = daily.join("skills/custom-skill");
    for relative in [
        "SKILL.md",
        ".git/config",
        ".cache/index",
        "cache/blob.bin",
        "node_modules/dependency.js",
        "target/debug.bin",
        "__pycache__/module.pyc",
        "notes.tmp",
        "draft.swp",
        "backup~",
        ".DS_Store",
    ] {
        let path = skill.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, relative).unwrap();
    }

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 1);
    assert!(isolated.join("skills/custom-skill/SKILL.md").is_file());
    for relative in [
        ".git/config",
        ".cache/index",
        "cache/blob.bin",
        "node_modules/dependency.js",
        "target/debug.bin",
        "__pycache__/module.pyc",
        "notes.tmp",
        "draft.swp",
        "backup~",
        ".DS_Store",
    ] {
        assert!(!isolated.join("skills/custom-skill").join(relative).exists());
    }
}

#[test]
fn hard_linked_daily_skill_files_are_excluded() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Hard linked\n").unwrap();
    fs::hard_link(&source, temp.path().join("second-link.md")).unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 0);
    assert_eq!(receipt.skipped, 1);
    assert!(!isolated.join("skills/custom-skill/SKILL.md").exists());
}

#[test]
fn a_reparse_backed_daily_skills_root_is_rejected() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let external = temp.path().join("external-skills");
    fs::create_dir_all(external.join("custom-skill")).unwrap();
    fs::write(external.join("custom-skill/SKILL.md"), "# External\n").unwrap();
    fs::create_dir_all(&daily).unwrap();
    symlink_dir(&external, daily.join("skills")).unwrap();

    let error = sync_native_skills(&daily, &isolated).unwrap_err();

    assert!(error.to_string().contains("reparse point"));
    assert!(!isolated.join("skills/custom-skill/SKILL.md").exists());
}

#[test]
fn a_reparse_backed_isolated_destination_is_preserved_without_following_it() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    let external = temp.path().join("external.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&source, "# Daily\n").unwrap();
    fs::write(&external, "# External\n").unwrap();
    symlink_file(&external, &destination).unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.projected, 0);
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(fs::read_to_string(&external).unwrap(), "# External\n");
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    let conflict = &manifest["conflicts"]["custom-skill/SKILL.md"];
    assert_eq!(conflict["reason"], "destination_reparse_point");
    assert!(conflict["destination_sha256"].is_null());
}

#[test]
fn a_hard_linked_isolated_projection_is_preserved_for_review() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    let second_link = temp.path().join("second-link.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Version one\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();
    fs::hard_link(&destination, &second_link).unwrap();
    fs::write(&source, "# Version two\n").unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.updated, 0);
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(fs::read_to_string(&destination).unwrap(), "# Version one\n");
    assert_eq!(fs::read_to_string(&second_link).unwrap(), "# Version one\n");
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["conflicts"]["custom-skill/SKILL.md"]["reason"],
        "destination_hard_link"
    );
}

#[test]
fn manifest_paths_cannot_escape_the_isolated_skills_root() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    fs::create_dir_all(daily.join("skills")).unwrap();
    fs::create_dir_all(isolated.join("skills")).unwrap();
    let outside = isolated.join("outside.md");
    let content = b"outside\n";
    fs::write(&outside, content).unwrap();
    let sha256 = format!("{:x}", Sha256::digest(content));
    fs::write(
        isolated.join("skill-projection-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "files": {
                "../outside.md": {
                    "source_sha256": sha256,
                    "destination_sha256": sha256
                }
            },
            "conflicts": {}
        }))
        .unwrap(),
    )
    .unwrap();

    let error = sync_native_skills(&daily, &isolated).unwrap_err();

    assert!(error.to_string().contains("invalid relative path"));
    assert_eq!(fs::read(&outside).unwrap(), content);
}

#[test]
fn a_managed_projection_is_preserved_when_its_source_becomes_hard_linked() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let source = daily.join("skills/custom-skill/SKILL.md");
    let destination = isolated.join("skills/custom-skill/SKILL.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "# Trusted version\n").unwrap();
    sync_native_skills(&daily, &isolated).unwrap();
    fs::hard_link(&source, temp.path().join("source-second-link.md")).unwrap();

    let receipt = sync_native_skills(&daily, &isolated).unwrap();

    assert_eq!(receipt.removed, 0);
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(receipt.skipped, 1);
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        "# Trusted version\n"
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(isolated.join("skill-projection-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["conflicts"]["custom-skill/SKILL.md"]["reason"],
        "source_hard_link"
    );
}
