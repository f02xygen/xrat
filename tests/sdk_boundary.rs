use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, thread, time::Duration};
fn output(command: &mut Command) -> Vec<u8> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn check_dependencies(directory: &Path) {
    let tree = output(
        Command::new("cargo")
            .args([
                "tree", "--locked", "-p", "xrat-sdk", "--edges", "normal", "--prefix", "none",
                "--format", "{p}",
            ])
            .current_dir(directory),
    );
    let tree = String::from_utf8(tree).unwrap();
    let forbidden = [
        "xrat-app",
        "xrat-db",
        "clap",
        "ratatui",
        "crossterm",
        "sqlx",
        "arboard",
        "axum",
        "tonic",
        "prost",
    ];
    for name in tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
    {
        assert!(!forbidden.contains(&name), "default SDK includes {name}");
    }
}
fn registry_dependencies(packages: &[Value], members: &[Value]) -> bool {
    packages.iter().all(|package| {
        !package["name"].as_str().unwrap().starts_with("xrat-")
            || members.contains(&package["id"])
            || package["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("registry+"))
    })
}
#[test]
fn default_sdk_excludes_application_dependencies() {
    check_dependencies(Path::new(env!("CARGO_MANIFEST_DIR")));
}
#[test]
fn registry_consumer_allows_local_root_and_rejects_internal_path_or_git_dependencies() {
    assert!(registry_dependencies(
        &[
            json!({"id":"consumer","name":"xrat-sdk-consumer","source":null}),
            json!({"id":"sdk","name":"xrat-sdk","source":"registry+https://github.com/rust-lang/crates.io-index"})
        ],
        &[json!("consumer")]
    ));
    for source in [Value::Null, json!("git+https://github.com/mhyrzt/xrat")] {
        assert!(!registry_dependencies(
            &[json!({"id":"config","name":"xrat-config","source":source})],
            &[json!("consumer")]
        ));
    }
}
#[test]
#[ignore = "builds a standalone Cargo consumer; registry verification requires network access"]
fn standalone_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path();
    fs::create_dir(destination.join("src")).unwrap();
    fs::copy(
        root.join("testdata/sdk-consumer/src/main.rs"),
        destination.join("src/main.rs"),
    )
    .unwrap();
    let manifest = destination.join("Cargo.toml");
    let original = fs::read_to_string(root.join("testdata/sdk-consumer/Cargo.toml")).unwrap();
    let dependency = "xrat-sdk = { path = \"../../crates/xrat-sdk\" }";
    let registry_version = std::env::var("XRAT_SDK_REGISTRY_VERSION").ok();
    if registry_version.is_some() {
        fs::write(&manifest, original.replace(&format!("{dependency}\n"), "")).unwrap();
        for attempt in 0..5 {
            let status = Command::new("cargo")
                .args(["add", "xrat-sdk"])
                .current_dir(destination)
                .status()
                .unwrap();
            if status.success() {
                break;
            }
            assert!(attempt < 4, "cargo add xrat-sdk failed");
            thread::sleep(Duration::from_secs(20));
        }
    } else {
        fs::write(
            &manifest,
            original.replace(
                dependency,
                &format!(
                    "xrat-sdk = {{ path = {} }}",
                    json!(root.join("crates/xrat-sdk"))
                ),
            ),
        )
        .unwrap();
        fs::copy(root.join("Cargo.lock"), destination.join("Cargo.lock")).unwrap();
    }
    let status = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .current_dir(destination)
        .status()
        .unwrap();
    assert!(status.success(), "standalone consumer failed");
    check_dependencies(destination);
    let metadata: Value = serde_json::from_slice(&output(
        Command::new("cargo")
            .args(["metadata", "--format-version", "1"])
            .current_dir(destination),
    ))
    .unwrap();
    let sdk = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "xrat-sdk")
        .unwrap();
    if let Some(version) = registry_version {
        assert_eq!(sdk["version"], version);
        assert!(sdk["source"].as_str().unwrap().starts_with("registry+"));
        assert!(registry_dependencies(
            metadata["packages"].as_array().unwrap(),
            metadata["workspace_members"].as_array().unwrap()
        ));
    }
    println!("Standalone consumer verified: xrat-sdk {}", sdk["version"]);
}
