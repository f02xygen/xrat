use std::{fs, path::Path, process::Command};
use toml_edit::DocumentMut;

fn versions_match(content: &str) -> bool {
    let manifest: DocumentMut = content.parse().unwrap();
    let current = manifest["workspace"]["package"]["version"]
        .as_str()
        .unwrap();
    manifest["workspace"]["dependencies"]
        .as_table_like()
        .unwrap()
        .iter()
        .all(|(_, dependency)| {
            dependency.get("path").is_none()
                || dependency.get("version").and_then(|value| value.as_str()) == Some(current)
        })
}
#[test]
fn workspace_versions_match() {
    assert!(
        versions_match(include_str!("../Cargo.toml")),
        "workspace internal dependency versions differ"
    );
}
#[test]
fn detects_unsynchronized_versions() {
    assert!(!versions_match(
        "[workspace.package]\nversion=\"0.21.0\"\n[workspace.dependencies]\nxrat-model={path=\"model\",version=\"0.20.0\"}"
    ));
}
#[test]
fn version_recipe_updates_versions_and_preserves_unrelated_content() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "").unwrap();
    fs::create_dir_all(root.path().join("model/src")).unwrap();
    fs::write(root.path().join("model/src/lib.rs"), "").unwrap();
    fs::write(
        root.path().join("model/Cargo.toml"),
        "[package]\nname=\"xrat-model\"\nversion.workspace=true\nedition.workspace=true\n",
    )
    .unwrap();
    let manifest = root.path().join("Cargo.toml");
    fs::write(&manifest, "# retained\n[package]\nname=\"fixture\"\nversion.workspace=true\nedition.workspace=true\n[workspace]\nmembers=[\"model\"]\n[workspace.package]\nversion = \"0.20.0\"\nedition=\"2024\"\n[workspace.dependencies]\nxrat-model = { path = \"model\", version = \"0.20.0\" }\nserde = \"1.0\"\n").unwrap();
    let recipe = Path::new(env!("CARGO_MANIFEST_DIR")).join("Justfile");
    let run = |version| {
        Command::new("just")
            .env("XDG_RUNTIME_DIR", root.path())
            .arg("--justfile")
            .arg(&recipe)
            .arg("--working-directory")
            .arg(root.path())
            .args(["set-version", version])
            .output()
            .unwrap()
    };
    let result = run("0.21.0");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let content = fs::read_to_string(&manifest).unwrap();
    assert!(content.starts_with("# retained"));
    assert!(content.contains("serde = \"1.0\""));
    assert_eq!(content.matches("0.21.0").count(), 2);
    assert!(versions_match(&content));
    assert!(!run("invalid").status.success());
    assert_eq!(fs::read_to_string(manifest).unwrap(), content);
}
