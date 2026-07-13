use std::{fs, path::Path};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn obsolete_alternate_runtime_files_are_absent() {
    for relative in [
        "assets/ui-app.js",
        "assets/ui.html",
        concat!("src/com", "panion.rs"),
        "src/jsonl.rs",
        "src/mode.rs",
        "src/runtime_client.rs",
        "src/runtime_process.rs",
    ] {
        assert!(
            !root().join(relative).exists(),
            "obsolete path remains: {relative}"
        );
    }
}

#[test]
fn active_repository_surfaces_describe_only_model_list_injection() {
    let surfaces = [
        "Cargo.toml",
        "README.md",
        "SECURITY.md",
        "docs/ARCHITECTURE.md",
        "docs/COMPATIBILITY.md",
        "docs/HOST_ADAPTERS.md",
        "docs/UPDATE_ISOLATION.md",
        ".agent_memory/MEMORY.md",
        ".agent_memory/decisions/model-list-injection-boundary.md",
        ".agent_memory/decisions/isolated-official-desktop-instance.md",
        ".agent_memory/decisions/update-isolation-contract.md",
    ];
    let forbidden = [
        concat!("Grok ", "Build"),
        concat!("Grok ", "CLI"),
        concat!("A", "CP"),
        concat!("dual-main", "-agent"),
        concat!("launch", "-native"),
        concat!("com", "panion"),
        concat!("independent Grok ", "UI"),
    ];

    for relative in surfaces {
        let content = fs::read_to_string(root().join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for term in forbidden {
            assert!(
                !content.contains(term),
                "obsolete term {term:?} remains in {relative}"
            );
        }
    }
}
