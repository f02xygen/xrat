use std::path::{Path, PathBuf};

use crate::app::AppError;
use crate::app::context::AppContext;
use crate::cli::{DaemonInstallArgs, DaemonUninstallArgs};

fn resolve_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/local/bin/xrat"))
}

fn render_service(template: &str, exe: &Path, xrat_path: &str) -> String {
    template
        .replace("{{EXE}}", &exe.display().to_string())
        .replace("{{XRAT_PATH}}", xrat_path)
}

/// Print a progress line unless `quiet` (used when `setup` drives the install
/// and renders its own step summary).
#[allow(dead_code)]
fn say(quiet: bool, line: String) {
    if !quiet {
        println!("{line}");
    }
}

// ---------------------------------------------------------------------------
// Linux: systemd user services
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub const DAEMON_SERVICE_NAME: &str = "xrat-daemon.service";
#[cfg(target_os = "linux")]
pub const DAEMON_OVERRIDE_DIR_NAME: &str = "xrat-daemon.service.d";
#[cfg(target_os = "linux")]
pub const DAEMON_TUN_OVERRIDE_FILE_NAME: &str = "10-tun.conf";
#[cfg(target_os = "linux")]
pub const DAEMON_TUN_OVERRIDE_TEMPLATE: &str =
    include_str!("../../../templates/systemd/xrat-daemon-tun.override.template");
#[cfg(target_os = "linux")]
const API_SERVICE_NAME: &str = "xrat-api.service";

#[cfg(target_os = "linux")]
const DAEMON_SERVICE_TEMPLATE: &str =
    include_str!("../../../templates/systemd/xrat-daemon.service.template");
#[cfg(target_os = "linux")]
const API_SERVICE_TEMPLATE: &str =
    include_str!("../../../templates/systemd/xrat-api.service.template");

#[cfg(target_os = "linux")]
pub fn systemd_user_dir_with_env(
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<PathBuf> {
    let base = env
        .get_os(("XDG_CONFIG_HOME").as_ref())
        .map(PathBuf::from)
        .or_else(|| {
            env.get_os(("HOME").as_ref())
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .ok_or(AppError::MissingHomeDirectory)?;
    Ok(base.join("systemd").join("user"))
}

#[cfg(target_os = "linux")]
fn generate_daemon_service(exe: &Path, xrat_path: &str) -> String {
    render_service(DAEMON_SERVICE_TEMPLATE, exe, xrat_path)
}

#[cfg(target_os = "linux")]
fn generate_api_service(exe: &Path, xrat_path: &str) -> String {
    render_service(API_SERVICE_TEMPLATE, exe, xrat_path)
}

#[cfg(target_os = "linux")]
fn run_systemctl_with_spawner(
    args: &[&str],
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    let status = xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
        .arg("--user")
        .args(args)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "systemctl --user {} exited with {}",
            args.join(" "),
            status
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn systemctl_available_with_spawner(
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> bool {
    xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
        .args(["--user", "status"])
        .stdout(xrat_support::process::Stdio::null())
        .stderr(xrat_support::process::Stdio::null())
        .status()
        .is_ok()
}

#[cfg(target_os = "linux")]
pub fn install(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
) -> crate::app::Result<()> {
    install_with_spawner(
        context,
        args,
        quiet,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(target_os = "linux")]
pub fn install_with_spawner(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    install_with_dependencies(
        context,
        args,
        quiet,
        spawner,
        &xrat_support::env::SystemEnvVars,
    )
}

#[cfg(target_os = "linux")]
pub fn install_with_dependencies(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<()> {
    if !args.dry_run && !systemctl_available_with_spawner(spawner.clone()) {
        return Err(AppError::InvalidArgument(
            "systemd is not available; ensure `systemctl --user status` works".to_string(),
        ));
    }

    let exe = resolve_exe();
    let xrat_path = context.runtime_paths.root_dir.display().to_string();
    let service_dir = systemd_user_dir_with_env(env)?;

    let daemon_content = generate_daemon_service(&exe, &xrat_path);
    let api_content = generate_api_service(&exe, &xrat_path);
    let daemon_path = service_dir.join(DAEMON_SERVICE_NAME);
    let api_path = service_dir.join(API_SERVICE_NAME);

    let install_tun_override = args.tun || context.app_config.runtime.tun.enabled;
    let override_dir = service_dir.join(DAEMON_OVERRIDE_DIR_NAME);
    let override_path = override_dir.join(DAEMON_TUN_OVERRIDE_FILE_NAME);

    if args.dry_run {
        println!("--- dry run: no files written ---\n");
        println!("Service directory: {}", service_dir.display());
        println!();
        println!("--- {} ---", DAEMON_SERVICE_NAME);
        print!("{daemon_content}");
        if install_tun_override {
            println!();
            println!(
                "--- {}/{} ---",
                DAEMON_OVERRIDE_DIR_NAME, DAEMON_TUN_OVERRIDE_FILE_NAME
            );
            print!("{DAEMON_TUN_OVERRIDE_TEMPLATE}");
        }
        if args.with_api {
            println!();
            println!("--- {} ---", API_SERVICE_NAME);
            print!("{api_content}");
        }
        println!();
        println!("Actions that would run:");
        println!("  systemctl --user daemon-reload");
        println!("  systemctl --user enable {DAEMON_SERVICE_NAME}");
        if args.with_api {
            println!("  systemctl --user enable {API_SERVICE_NAME}");
        }
        if args.start {
            println!("  systemctl --user start {DAEMON_SERVICE_NAME}");
        }
        return Ok(());
    }

    std::fs::create_dir_all(&service_dir)?;

    std::fs::write(&daemon_path, &daemon_content)?;
    say(quiet, format!("Written: {}", daemon_path.display()));

    if install_tun_override {
        std::fs::create_dir_all(&override_dir)?;
        std::fs::write(&override_path, DAEMON_TUN_OVERRIDE_TEMPLATE)?;
        say(quiet, format!("Written: {}", override_path.display()));
    }

    if args.with_api {
        std::fs::write(&api_path, &api_content)?;
        say(quiet, format!("Written: {}", api_path.display()));
    }

    run_systemctl_with_spawner(&["daemon-reload"], spawner.clone())?;
    say(quiet, "Reloaded systemd user daemon.".to_string());

    run_systemctl_with_spawner(&["enable", DAEMON_SERVICE_NAME], spawner.clone())?;
    say(quiet, format!("Enabled: {DAEMON_SERVICE_NAME}"));

    if args.with_api {
        run_systemctl_with_spawner(&["enable", API_SERVICE_NAME], spawner.clone())?;
        say(quiet, format!("Enabled: {API_SERVICE_NAME}"));
    }

    if args.start {
        run_systemctl_with_spawner(&["start", DAEMON_SERVICE_NAME], spawner.clone())?;
        say(quiet, format!("Started: {DAEMON_SERVICE_NAME}"));
    }

    if !quiet {
        println!();
        println!("Daemon installed successfully.");
        if !args.start {
            println!("Start with: xrat daemon start");
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall(_context: &AppContext, args: &DaemonUninstallArgs) -> crate::app::Result<()> {
    uninstall_with_spawner(
        _context,
        args,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(target_os = "linux")]
pub fn uninstall_with_spawner(
    _context: &AppContext,
    args: &DaemonUninstallArgs,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    uninstall_with_dependencies(_context, args, spawner, &xrat_support::env::SystemEnvVars)
}

#[cfg(target_os = "linux")]
pub fn uninstall_with_dependencies(
    _context: &AppContext,
    args: &DaemonUninstallArgs,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<()> {
    let service_dir = systemd_user_dir_with_env(env)?;
    let daemon_path = service_dir.join(DAEMON_SERVICE_NAME);
    let api_path = service_dir.join(API_SERVICE_NAME);
    let override_dir = service_dir.join(DAEMON_OVERRIDE_DIR_NAME);
    let override_path = override_dir.join(DAEMON_TUN_OVERRIDE_FILE_NAME);

    if args.dry_run {
        println!("--- dry run: no files removed ---\n");
        if daemon_path.exists() {
            println!("Would stop:    systemctl --user stop {DAEMON_SERVICE_NAME}");
            println!("Would disable: systemctl --user disable {DAEMON_SERVICE_NAME}");
            println!("Would remove:  {}", daemon_path.display());
        } else {
            println!("Not present: {}", daemon_path.display());
        }
        if override_path.exists() {
            println!("Would remove:  {}", override_path.display());
        }
        if api_path.exists() {
            println!("Would stop:    systemctl --user stop {API_SERVICE_NAME}");
            println!("Would disable: systemctl --user disable {API_SERVICE_NAME}");
            println!("Would remove:  {}", api_path.display());
        }
        println!("Would run: systemctl --user daemon-reload");
        return Ok(());
    }

    let mut removed = false;

    if override_path.exists() {
        std::fs::remove_file(&override_path)?;
        let _ = std::fs::remove_dir(&override_dir);
        println!("Removed: {}", override_path.display());
        removed = true;
    }

    if daemon_path.exists() {
        let _ = xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
            .args(["--user", "stop", DAEMON_SERVICE_NAME])
            .status();
        let _ = xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
            .args(["--user", "disable", DAEMON_SERVICE_NAME])
            .status();
        std::fs::remove_file(&daemon_path)?;
        println!("Removed: {}", daemon_path.display());
        removed = true;
    } else {
        println!("Not present: {}", daemon_path.display());
    }

    if api_path.exists() {
        let _ = xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
            .args(["--user", "stop", API_SERVICE_NAME])
            .status();
        let _ = xrat_support::process::Command::with_spawner("systemctl", spawner.clone())
            .args(["--user", "disable", API_SERVICE_NAME])
            .status();
        std::fs::remove_file(&api_path)?;
        println!("Removed: {}", api_path.display());
        removed = true;
    }

    if removed {
        run_systemctl_with_spawner(&["daemon-reload"], spawner.clone())?;
        println!("Reloaded systemd user daemon.");
    }

    println!();
    println!("Daemon uninstalled. Config and data preserved.");

    Ok(())
}

// ---------------------------------------------------------------------------
// macOS: launchd user agents
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
const DAEMON_LABEL: &str = "com.xrat.daemon";
#[cfg(target_os = "macos")]
const API_LABEL: &str = "com.xrat.api";
#[cfg(target_os = "macos")]
const DAEMON_PLIST_NAME: &str = "com.xrat.daemon.plist";
#[cfg(target_os = "macos")]
const API_PLIST_NAME: &str = "com.xrat.api.plist";

#[cfg(target_os = "macos")]
const DAEMON_PLIST_TEMPLATE: &str =
    include_str!("../../../templates/launchd/xrat-daemon.plist.template");
#[cfg(target_os = "macos")]
const API_PLIST_TEMPLATE: &str = include_str!("../../../templates/launchd/xrat-api.plist.template");

#[cfg(target_os = "macos")]
fn launchd_agents_dir_with_env(
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<PathBuf> {
    let home = env
        .get_os(("HOME").as_ref())
        .ok_or(AppError::MissingHomeDirectory)?;
    Ok(PathBuf::from(home).join("Library").join("LaunchAgents"))
}

#[cfg(target_os = "macos")]
fn current_uid_with_spawner(
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<String> {
    let output = xrat_support::process::Command::with_spawner("id", spawner.clone())
        .arg("-u")
        .output()
        .map_err(|err| AppError::InvalidArgument(format!("failed to run `id -u`: {err}")))?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() {
        return Err(AppError::InvalidArgument(
            "could not determine current uid".to_string(),
        ));
    }
    Ok(uid)
}

#[cfg(target_os = "macos")]
fn launchctl_available_with_spawner(
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> bool {
    xrat_support::process::Command::with_spawner("launchctl", spawner.clone())
        .arg("help")
        .stdout(xrat_support::process::Stdio::null())
        .stderr(xrat_support::process::Stdio::null())
        .status()
        .is_ok()
}

#[cfg(target_os = "macos")]
fn run_launchctl_with_spawner(
    args: &[&str],
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    let status = xrat_support::process::Command::with_spawner("launchctl", spawner.clone())
        .args(args)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "launchctl {} exited with {}",
            args.join(" "),
            status
        )));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn generate_daemon_plist(exe: &Path, xrat_path: &str) -> String {
    render_service(DAEMON_PLIST_TEMPLATE, exe, xrat_path)
}

#[cfg(target_os = "macos")]
fn generate_api_plist(exe: &Path, xrat_path: &str) -> String {
    render_service(API_PLIST_TEMPLATE, exe, xrat_path)
}

#[cfg(target_os = "macos")]
pub fn install(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
) -> crate::app::Result<()> {
    install_with_spawner(
        context,
        args,
        quiet,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(target_os = "macos")]
pub fn install_with_spawner(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    install_with_dependencies(
        context,
        args,
        quiet,
        spawner,
        &xrat_support::env::SystemEnvVars,
    )
}

#[cfg(target_os = "macos")]
pub fn install_with_dependencies(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<()> {
    if !args.dry_run && !launchctl_available_with_spawner(spawner.clone()) {
        return Err(AppError::InvalidArgument(
            "launchd is not available; this requires macOS with launchctl".to_string(),
        ));
    }

    let exe = resolve_exe();
    let xrat_path = context.runtime_paths.root_dir.display().to_string();
    let agents_dir = launchd_agents_dir_with_env(env)?;

    let daemon_content = generate_daemon_plist(&exe, &xrat_path);
    let api_content = generate_api_plist(&exe, &xrat_path);
    let daemon_path = agents_dir.join(DAEMON_PLIST_NAME);
    let api_path = agents_dir.join(API_PLIST_NAME);

    if args.dry_run {
        println!("--- dry run: no files written ---\n");
        println!("LaunchAgents directory: {}", agents_dir.display());
        println!();
        println!("--- {DAEMON_PLIST_NAME} ---");
        print!("{daemon_content}");
        if args.with_api {
            println!();
            println!("--- {API_PLIST_NAME} ---");
            print!("{api_content}");
        }
        println!();
        println!("Actions that would run:");
        println!(
            "  launchctl bootstrap gui/$(id -u) {}",
            daemon_path.display()
        );
        if args.with_api {
            println!("  launchctl bootstrap gui/$(id -u) {}", api_path.display());
        }
        if args.start {
            println!("  launchctl kickstart -k gui/$(id -u)/{DAEMON_LABEL}");
        }
        return Ok(());
    }

    std::fs::create_dir_all(&agents_dir)?;

    std::fs::write(&daemon_path, &daemon_content)?;
    say(quiet, format!("Written: {}", daemon_path.display()));

    if args.with_api {
        std::fs::write(&api_path, &api_content)?;
        say(quiet, format!("Written: {}", api_path.display()));
    }

    let uid = current_uid_with_spawner(spawner.clone())?;
    let domain = format!("gui/{uid}");

    run_launchctl_with_spawner(
        &["bootstrap", &domain, &daemon_path.display().to_string()],
        spawner.clone(),
    )?;
    say(quiet, format!("Bootstrapped: {DAEMON_LABEL}"));

    if args.with_api {
        run_launchctl_with_spawner(
            &["bootstrap", &domain, &api_path.display().to_string()],
            spawner.clone(),
        )?;
        say(quiet, format!("Bootstrapped: {API_LABEL}"));
    }

    if args.start {
        run_launchctl_with_spawner(
            &["kickstart", "-k", &format!("{domain}/{DAEMON_LABEL}")],
            spawner.clone(),
        )?;
        say(quiet, format!("Started: {DAEMON_LABEL}"));
    }

    if !quiet {
        println!();
        println!("Daemon installed successfully.");
        if !args.start {
            println!("Start with: xrat daemon start");
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall(_context: &AppContext, args: &DaemonUninstallArgs) -> crate::app::Result<()> {
    uninstall_with_spawner(
        _context,
        args,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(target_os = "macos")]
pub fn uninstall_with_spawner(
    _context: &AppContext,
    args: &DaemonUninstallArgs,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    uninstall_with_dependencies(_context, args, spawner, &xrat_support::env::SystemEnvVars)
}

#[cfg(target_os = "macos")]
pub fn uninstall_with_dependencies(
    _context: &AppContext,
    args: &DaemonUninstallArgs,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
    env: &dyn xrat_support::env::EnvVars,
) -> crate::app::Result<()> {
    let agents_dir = launchd_agents_dir_with_env(env)?;
    let daemon_path = agents_dir.join(DAEMON_PLIST_NAME);
    let api_path = agents_dir.join(API_PLIST_NAME);

    if args.dry_run {
        println!("--- dry run: no files removed ---\n");
        if daemon_path.exists() {
            println!("Would bootout: launchctl bootout gui/$(id -u)/{DAEMON_LABEL}");
            println!("Would remove:  {}", daemon_path.display());
        } else {
            println!("Not present: {}", daemon_path.display());
        }
        if api_path.exists() {
            println!("Would bootout: launchctl bootout gui/$(id -u)/{API_LABEL}");
            println!("Would remove:  {}", api_path.display());
        }
        return Ok(());
    }

    let uid = current_uid_with_spawner(spawner.clone())?;
    let domain = format!("gui/{uid}");

    if daemon_path.exists() {
        let _ = xrat_support::process::Command::with_spawner("launchctl", spawner.clone())
            .args(["bootout", &format!("{domain}/{DAEMON_LABEL}")])
            .status();
        std::fs::remove_file(&daemon_path)?;
        println!("Removed: {}", daemon_path.display());
    } else {
        println!("Not present: {}", daemon_path.display());
    }

    if api_path.exists() {
        let _ = xrat_support::process::Command::with_spawner("launchctl", spawner.clone())
            .args(["bootout", &format!("{domain}/{API_LABEL}")])
            .status();
        std::fs::remove_file(&api_path)?;
        println!("Removed: {}", api_path.display());
    }

    println!();
    println!("Daemon uninstalled. Config and data preserved.");

    Ok(())
}

// ---------------------------------------------------------------------------
// FreeBSD / OpenBSD: rc.d scripts
//
// rc.d scripts live in a system directory and the enable/start commands
// (sysrc/service on FreeBSD, rcctl on OpenBSD) require root. Run under sudo or
// as root.
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
const DAEMON_RC_NAME: &str = "xrat_daemon";
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
const API_RC_NAME: &str = "xrat_api";

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
const DAEMON_RC_TEMPLATE: &str = include_str!("../../../templates/rc.d/xrat_daemon.template");
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
const API_RC_TEMPLATE: &str = include_str!("../../../templates/rc.d/xrat_api.template");

#[cfg(target_os = "freebsd")]
fn rc_d_dir() -> PathBuf {
    PathBuf::from("/usr/local/etc/rc.d")
}
#[cfg(target_os = "openbsd")]
fn rc_d_dir() -> PathBuf {
    PathBuf::from("/etc/rc.d")
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn generate_daemon_rc(exe: &Path, xrat_path: &str) -> String {
    render_service(DAEMON_RC_TEMPLATE, exe, xrat_path)
}
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn generate_api_rc(exe: &Path, xrat_path: &str) -> String {
    render_service(API_RC_TEMPLATE, exe, xrat_path)
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn run_status_with_spawner(
    cmd: &str,
    args: &[&str],
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    let status = xrat_support::process::Command::with_spawner(cmd, spawner.clone())
        .args(args)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "{cmd} {} exited with {}",
            args.join(" "),
            status
        )));
    }
    Ok(())
}

#[cfg(target_os = "freebsd")]
fn enable_service(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    run_status_with_spawner("sysrc", &[&format!("{rc_name}_enable=YES")], spawner)
}
#[cfg(target_os = "freebsd")]
fn start_service(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    run_status_with_spawner("service", &[rc_name, "start"], spawner)
}
#[cfg(target_os = "freebsd")]
fn stop_and_disable_service_with_spawner(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) {
    let _ = xrat_support::process::Command::with_spawner("service", spawner.clone())
        .args([rc_name, "stop"])
        .status();
    let _ = xrat_support::process::Command::with_spawner("sysrc", spawner.clone())
        .arg(format!("{rc_name}_enable=NO"))
        .status();
}

#[cfg(target_os = "openbsd")]
fn enable_service(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    run_status_with_spawner("rcctl", &["enable", rc_name], spawner)
}
#[cfg(target_os = "openbsd")]
fn start_service(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> std::io::Result<()> {
    run_status_with_spawner("rcctl", &["start", rc_name], spawner)
}
#[cfg(target_os = "openbsd")]
fn stop_and_disable_service_with_spawner(
    rc_name: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) {
    let _ = xrat_support::process::Command::with_spawner("rcctl", spawner.clone())
        .args(["stop", rc_name])
        .status();
    let _ = xrat_support::process::Command::with_spawner("rcctl", spawner.clone())
        .args(["disable", rc_name])
        .status();
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn write_rc_script(path: &Path, content: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub fn install(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
) -> crate::app::Result<()> {
    install_with_spawner(
        context,
        args,
        quiet,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub fn install_with_spawner(
    context: &AppContext,
    args: &DaemonInstallArgs,
    quiet: bool,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    let exe = resolve_exe();
    let xrat_path = context.runtime_paths.root_dir.display().to_string();
    let dir = rc_d_dir();

    let daemon_content = generate_daemon_rc(&exe, &xrat_path);
    let api_content = generate_api_rc(&exe, &xrat_path);
    let daemon_path = dir.join(DAEMON_RC_NAME);
    let api_path = dir.join(API_RC_NAME);

    if args.dry_run {
        println!("--- dry run: no files written ---\n");
        println!("rc.d directory: {}", dir.display());
        println!();
        println!("--- {DAEMON_RC_NAME} ---");
        print!("{daemon_content}");
        if args.with_api {
            println!();
            println!("--- {API_RC_NAME} ---");
            print!("{api_content}");
        }
        println!();
        println!("Actions that would run (require root):");
        println!("  enable service {DAEMON_RC_NAME}");
        if args.with_api {
            println!("  enable service {API_RC_NAME}");
        }
        if args.start {
            println!("  start service {DAEMON_RC_NAME}");
        }
        return Ok(());
    }

    std::fs::create_dir_all(&dir)?;

    write_rc_script(&daemon_path, &daemon_content)?;
    say(quiet, format!("Written: {}", daemon_path.display()));

    if args.with_api {
        write_rc_script(&api_path, &api_content)?;
        say(quiet, format!("Written: {}", api_path.display()));
    }

    enable_service(DAEMON_RC_NAME, spawner.clone())?;
    say(quiet, format!("Enabled: {DAEMON_RC_NAME}"));

    if args.with_api {
        enable_service(API_RC_NAME, spawner.clone())?;
        say(quiet, format!("Enabled: {API_RC_NAME}"));
    }

    if args.start {
        start_service(DAEMON_RC_NAME, spawner.clone())?;
        say(quiet, format!("Started: {DAEMON_RC_NAME}"));
    }

    if !quiet {
        println!();
        println!("Daemon installed successfully.");
        if !args.start {
            println!("Start with: xrat daemon start");
        }
    }

    Ok(())
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub fn uninstall(_context: &AppContext, args: &DaemonUninstallArgs) -> crate::app::Result<()> {
    uninstall_with_spawner(
        _context,
        args,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub fn uninstall_with_spawner(
    _context: &AppContext,
    args: &DaemonUninstallArgs,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    let dir = rc_d_dir();
    let daemon_path = dir.join(DAEMON_RC_NAME);
    let api_path = dir.join(API_RC_NAME);

    if args.dry_run {
        println!("--- dry run: no files removed ---\n");
        if daemon_path.exists() {
            println!("Would stop and disable service {DAEMON_RC_NAME}");
            println!("Would remove: {}", daemon_path.display());
        } else {
            println!("Not present: {}", daemon_path.display());
        }
        if api_path.exists() {
            println!("Would stop and disable service {API_RC_NAME}");
            println!("Would remove: {}", api_path.display());
        }
        return Ok(());
    }

    if daemon_path.exists() {
        stop_and_disable_service_with_spawner(DAEMON_RC_NAME, spawner.clone());
        std::fs::remove_file(&daemon_path)?;
        println!("Removed: {}", daemon_path.display());
    } else {
        println!("Not present: {}", daemon_path.display());
    }

    if api_path.exists() {
        stop_and_disable_service_with_spawner(API_RC_NAME, spawner.clone());
        std::fs::remove_file(&api_path)?;
        println!("Removed: {}", api_path.display());
    }

    println!();
    println!("Daemon uninstalled. Config and data preserved.");

    Ok(())
}

// ---------------------------------------------------------------------------
// Unsupported platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd"
)))]
pub fn install(
    _context: &AppContext,
    _args: &DaemonInstallArgs,
    _quiet: bool,
) -> crate::app::Result<()> {
    Err(AppError::UnsupportedPlatform(
        "daemon install is not supported on this platform".to_string(),
    ))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd"
)))]
pub fn uninstall(_context: &AppContext, _args: &DaemonUninstallArgs) -> crate::app::Result<()> {
    Err(AppError::UnsupportedPlatform(
        "daemon uninstall is not supported on this platform".to_string(),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod process_tests {
    use super::*;
    use async_trait::async_trait;
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use std::sync::{Arc, Mutex};
    use xrat_support::process::{Child, CommandSpec, ProcessSpawner};

    struct ServiceSpawner(Mutex<Vec<Vec<std::ffi::OsString>>>);
    #[async_trait]
    impl ProcessSpawner for ServiceSpawner {
        fn spawn(&self, _: &CommandSpec) -> io::Result<Child> {
            panic!("service commands use status")
        }
        fn run(&self, spec: &CommandSpec, capture: bool) -> io::Result<Output> {
            assert_eq!(spec.program, "systemctl");
            assert!(!capture);
            self.0.lock().unwrap().push(spec.args.clone());
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
        async fn output_async(&self, _: &CommandSpec) -> io::Result<Output> {
            panic!("service commands use status")
        }
    }
    #[tokio::test]
    async fn injected_service_install_and_uninstall_never_call_real_systemctl() {
        let (context, root) = crate::app::tests::TestAppBuilder::new("service-ports")
            .build_with_root()
            .await;
        let env = xrat_support::env::MapEnvVars(std::collections::HashMap::from([(
            "XDG_CONFIG_HOME".into(),
            root.path().as_os_str().to_os_string(),
        )]));
        let spawner = Arc::new(ServiceSpawner(Mutex::new(Vec::new())));
        let install = DaemonInstallArgs {
            start: true,
            with_api: true,
            dry_run: false,
            tun: false,
        };
        install_with_dependencies(&context, &install, true, spawner.clone(), &env).unwrap();
        let service = root.path().join("systemd/user/xrat-daemon.service");
        assert!(service.exists());
        let uninstall = DaemonUninstallArgs { dry_run: false };
        uninstall_with_dependencies(&context, &uninstall, spawner.clone(), &env).unwrap();
        assert!(!service.exists());
        let actual: Vec<Vec<String>> = spawner
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|args| {
                args.iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect()
            })
            .collect();
        let expected = [
            vec!["--user", "status"],
            vec!["--user", "daemon-reload"],
            vec!["--user", "enable", DAEMON_SERVICE_NAME],
            vec!["--user", "enable", API_SERVICE_NAME],
            vec!["--user", "start", DAEMON_SERVICE_NAME],
            vec!["--user", "stop", DAEMON_SERVICE_NAME],
            vec!["--user", "disable", DAEMON_SERVICE_NAME],
            vec!["--user", "stop", API_SERVICE_NAME],
            vec!["--user", "disable", API_SERVICE_NAME],
            vec!["--user", "daemon-reload"],
        ];
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn tun_service_install_creates_and_removes_override() {
        let (context, root) = crate::app::tests::TestAppBuilder::new("service-tun")
            .build_with_root()
            .await;
        let env = xrat_support::env::MapEnvVars(std::collections::HashMap::from([(
            "XDG_CONFIG_HOME".into(),
            root.path().as_os_str().to_os_string(),
        )]));
        let spawner = Arc::new(ServiceSpawner(Mutex::new(Vec::new())));
        let install = DaemonInstallArgs {
            start: false,
            with_api: false,
            dry_run: false,
            tun: true,
        };
        install_with_dependencies(&context, &install, false, spawner.clone(), &env).unwrap();
        let override_file = root
            .path()
            .join("systemd/user/xrat-daemon.service.d/10-tun.conf");
        assert!(override_file.exists());
        let content = std::fs::read_to_string(&override_file).unwrap();
        assert!(content.contains("NoNewPrivileges=false"));
        assert!(content.contains("ProtectSystem=false"));
        assert!(content.contains("ProtectHome=false"));
        assert!(content.contains("PrivateTmp=false"));
        assert!(content.contains("ReadWritePaths="));

        let uninstall = DaemonUninstallArgs { dry_run: false };
        uninstall_with_dependencies(&context, &uninstall, spawner.clone(), &env).unwrap();
        assert!(!override_file.exists());
    }
}
