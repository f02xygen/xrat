use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunOwnershipRecord {
    pub interface_name: String,
    pub ifindex: Option<u32>,
    pub session_id: i64,
    pub engine: String,
    #[serde(default)]
    pub policy_rules: Vec<xrat_support::net::KernelPolicyRule>,
}

pub fn ownership_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("tun-ownership.json")
}

pub fn load_ownership(runtime_dir: &Path) -> Option<TunOwnershipRecord> {
    let path = ownership_path(runtime_dir);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_ownership(runtime_dir: &Path, record: &TunOwnershipRecord) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(runtime_dir)?;
    let path = ownership_path(runtime_dir);
    let temporary = runtime_dir.join(format!(".tun-ownership-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(record)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn clear_ownership(runtime_dir: &Path) {
    let path = ownership_path(runtime_dir);
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_ownership_record() {
        let temp = tempfile::tempdir().expect("temp dir");
        let runtime_dir = temp.path();

        assert_eq!(load_ownership(runtime_dir), None);

        let record = TunOwnershipRecord {
            interface_name: "xrat0".to_string(),
            ifindex: Some(42),
            session_id: 100,
            engine: "xray".to_string(),
            policy_rules: vec![],
        };
        save_ownership(runtime_dir, &record).expect("save");
        assert_eq!(load_ownership(runtime_dir), Some(record));

        clear_ownership(runtime_dir);
        assert_eq!(load_ownership(runtime_dir), None);
    }
}
