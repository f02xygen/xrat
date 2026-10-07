use std::io::Write;
use std::path::Path;
use std::time::Duration;
use xrat_support::process::Command;

use crate::app::AppError;
use crate::app::commands::output;
use crate::app::commands::progress::CliProgress;
use crate::cli::UpgradeArgs;

use super::{REPO, current_version, install_binary, run_post_upgrade_migrations, same_version};

pub(crate) async fn upgrade(
    args: &UpgradeArgs,
    target: &Path,
    config_path: &Path,
) -> crate::app::Result<()> {
    let color = output::color_enabled();
    let arch = detect_arch()?;

    let version = match &args.version {
        Some(tag) => tag.clone(),
        None => {
            let progress = CliProgress::spinner(true, "checking latest release");
            let result = fetch_latest_tag(args.timeout_secs).await;
            progress.finish_and_clear();
            result?
        }
    };

    if !args.force && same_version(&version, current_version()) {
        println!(
            "{}",
            output::success(
                format!("already using latest version ({})", current_version()),
                color
            )
        );
        return Ok(());
    }

    println!(
        "{}",
        output::notice(
            format!("upgrading {} -> {version}", current_version()),
            color
        )
    );

    let filename = format!("xrat-{version}-{arch}.tar.gz");
    let base_url = format!("https://github.com/{REPO}/releases/download/{version}");

    let work = tempfile::tempdir()?;
    let client = http_client(args.timeout_secs)?;

    download_with_progress(
        &client,
        &format!("{base_url}/{filename}"),
        &work.path().join(&filename),
        &filename,
    )
    .await?;

    let progress = CliProgress::spinner(true, "verifying checksum");
    let result = async {
        download(
            &client,
            &format!("{base_url}/SHASUMS256.txt"),
            &work.path().join("SHASUMS256.txt"),
        )
        .await?;
        verify_checksum(work.path(), &filename)
    }
    .await;
    progress.finish_and_clear();
    result?;

    let progress = CliProgress::spinner(true, format!("installing to {}", target.display()));
    let result = (|| {
        extract_binary(work.path(), &filename)?;
        install_binary(&work.path().join("xrat"), target)
    })();
    progress.finish_and_clear();
    result?;

    let progress = CliProgress::spinner(true, "applying database migrations");
    let result = run_post_upgrade_migrations(target, config_path);
    progress.finish_and_clear();
    result?;

    println!(
        "{}",
        output::success(
            format!("upgraded xrat {} -> {version}", current_version()),
            color
        )
    );

    Ok(())
}

fn detect_arch() -> crate::app::Result<&'static str> {
    detect_arch_with_platform(&xrat_support::platform::HostPlatformDetector)
}

fn detect_arch_with_platform(
    detector: &dyn xrat_support::platform::PlatformDetector,
) -> crate::app::Result<&'static str> {
    let platform = detector.detect();
    match (platform.os.as_str(), platform.arch.as_str()) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        (os, arch) => Err(AppError::UnsupportedPlatform(format!(
            "no prebuilt release for {os}/{arch}; use --source to build from source"
        ))),
    }
}

fn http_client(timeout_secs: u64) -> crate::app::Result<xrat_support::http::Client> {
    Ok(xrat_support::http::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(concat!("xrat/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

async fn fetch_latest_tag(timeout_secs: u64) -> crate::app::Result<String> {
    crate::app::services::releases::ReleaseService::default()
        .latest_tag(timeout_secs)
        .await
}

async fn download(
    client: &xrat_support::http::Client,
    url: &str,
    destination: &Path,
) -> crate::app::Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|source| AppError::ReleaseHttp {
            operation: "downloading xrat release checksums from github.com",
            source,
        })?;
    if !response.status().is_success() {
        return Err(AppError::InvalidArgument(format!(
            "download failed for {url}: HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|source| AppError::ReleaseHttp {
            operation: "reading xrat release checksums from github.com",
            source,
        })?;
    std::fs::write(destination, &bytes)?;
    Ok(())
}

async fn download_with_progress(
    client: &xrat_support::http::Client,
    url: &str,
    destination: &Path,
    label: &str,
) -> crate::app::Result<()> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|source| AppError::ReleaseHttp {
            operation: "downloading xrat release archive from github.com",
            source,
        })?;
    if !response.status().is_success() {
        return Err(AppError::InvalidArgument(format!(
            "download failed for {url}: HTTP {}",
            response.status()
        )));
    }

    let progress = CliProgress::bytes_bar(
        true,
        response.content_length(),
        format!("downloading {label}"),
    );
    let mut file = std::fs::File::create(destination)?;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|source| AppError::ReleaseHttp {
            operation: "reading xrat release archive from github.com",
            source,
        })?
    {
        file.write_all(&chunk)?;
        progress.inc(chunk.len() as u64);
    }
    file.flush()?;
    progress.finish_with_message(format!("{label} done"));
    Ok(())
}

fn verify_checksum(work_dir: &Path, filename: &str) -> crate::app::Result<()> {
    let sums = std::fs::read_to_string(work_dir.join("SHASUMS256.txt"))?;
    let line = sums
        .lines()
        .find(|line| line.contains(filename))
        .ok_or_else(|| AppError::InvalidArgument(format!("no checksum entry for {filename}")))?;
    std::fs::write(work_dir.join("checksum.txt"), format!("{line}\n"))?;

    run_in(
        work_dir,
        "sha256sum",
        &["-c", "checksum.txt"],
        "checksum verification failed",
    )
}

fn extract_binary(work_dir: &Path, filename: &str) -> crate::app::Result<()> {
    run_in(
        work_dir,
        "tar",
        &["-xzf", filename, "./xrat"],
        "failed to extract xrat binary from release archive",
    )
}

fn run_in(dir: &Path, program: &str, args: &[&str], context: &str) -> crate::app::Result<()> {
    run_in_with_spawner(
        dir,
        program,
        args,
        context,
        std::sync::Arc::new(xrat_support::process::SystemProcessSpawner),
    )
}

fn run_in_with_spawner(
    dir: &Path,
    program: &str,
    args: &[&str],
    context: &str,
    spawner: std::sync::Arc<dyn xrat_support::process::ProcessSpawner>,
) -> crate::app::Result<()> {
    let status = Command::with_spawner(program, spawner.clone())
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|error| {
            AppError::InvalidArgument(format!("{context}: cannot run {program}: {error}"))
        })?;
    if !status.success() {
        return Err(AppError::InvalidArgument(format!("{context} ({status})")));
    }
    Ok(())
}

#[cfg(test)]
mod platform_tests {
    use super::*;
    use xrat_support::platform::{Architecture, OperatingSystem, Platform};
    #[test]
    fn release_targets_use_injected_platform_and_reject_unsupported_pairs() {
        for (os, arch, target) in [
            (
                OperatingSystem::Linux,
                Architecture::X86_64,
                "x86_64-unknown-linux-musl",
            ),
            (
                OperatingSystem::Linux,
                Architecture::Aarch64,
                "aarch64-unknown-linux-musl",
            ),
            (
                OperatingSystem::Macos,
                Architecture::X86_64,
                "x86_64-apple-darwin",
            ),
            (
                OperatingSystem::Macos,
                Architecture::Aarch64,
                "aarch64-apple-darwin",
            ),
        ] {
            assert_eq!(
                detect_arch_with_platform(&Platform { os, arch }).unwrap(),
                target
            );
        }
        assert!(
            detect_arch_with_platform(&Platform {
                os: OperatingSystem::Windows,
                arch: Architecture::X86_64
            })
            .is_err()
        );
    }
}
