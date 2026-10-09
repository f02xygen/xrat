use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output},
    thread,
    time::{Duration, Instant},
};

pub fn path(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("set {variable}")));
    fs::canonicalize(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
pub fn directory(variable: &str) -> PathBuf {
    let path =
        PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("set {variable}")));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}
pub fn assert_isolated() {
    let parent =
        std::env::var_os("XRAT_TEST_PARENT_NETNS").expect("missing parent namespace identity");
    assert_ne!(
        fs::read_link("/proc/self/ns/net").unwrap(),
        PathBuf::from(parent),
        "refusing host network mutation"
    );
}
pub fn enter_namespace(test: &str) -> bool {
    if std::env::var("XRAT_TEST_NAMESPACE").as_deref() == Ok(test) {
        assert_isolated();
        return true;
    }
    let effective_uid = fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|uids| uids.split_whitespace().nth(1))
        .map(str::to_owned)
        .expect("missing effective UID");
    let arguments: &[&str] = if effective_uid == "0" {
        &["--net"]
    } else {
        &["--user", "--map-root-user", "--net"]
    };
    let status = Command::new("unshare")
        .args(arguments)
        .arg(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", test, "--nocapture"])
        .env("XRAT_TEST_NAMESPACE", test)
        .env(
            "XRAT_TEST_PARENT_NETNS",
            fs::read_link("/proc/self/ns/net").unwrap(),
        )
        .status()
        .expect("unshare must be installed and permitted");
    assert!(status.success(), "isolated {test} failed: {status}");
    false
}
pub fn checked(command: &mut Command) -> Output {
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{command:?}: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
pub fn ip(arguments: &[&str]) -> Value {
    let result = checked(Command::new("ip").args(arguments));
    if result.stdout.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&result.stdout).unwrap()
    }
}
pub fn wait_for(mut predicate: impl FnMut() -> bool, seconds: u64, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !predicate() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(100));
    }
}
pub fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
pub struct ChildGuard(pub Child);
impl ChildGuard {
    pub fn signal(&mut self, signal: &str) {
        if self.0.try_wait().unwrap().is_none() {
            checked(Command::new("kill").args([signal, &self.0.id().to_string()]));
        }
    }
    pub fn stop(&mut self) {
        self.signal("-TERM");
        self.wait(10);
    }
    pub fn wait(&mut self, seconds: u64) {
        wait_for(
            || self.0.try_wait().unwrap().is_some(),
            seconds,
            "child did not exit",
        );
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.0.id().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
