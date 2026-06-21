use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use semver::Version;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO_OWNER: &str = "nullshii";
const REPO_NAME: &str = "ShockingVRC";
const API_LATEST: &str =
    "https://api.github.com/repos/nullshii/ShockingVRC/releases/latest";

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

impl DownloadProgress {
    pub fn ratio(&self) -> f64 {
        match self.total {
            Some(total) if total > 0 => (self.downloaded as f64 / total as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    pub fn label(&self) -> String {
        let downloaded_mb = self.downloaded as f64 / (1024.0 * 1024.0);
        match self.total {
            Some(total) => {
                let total_mb = total as f64 / (1024.0 * 1024.0);
                format!("{downloaded_mb:.1} / {total_mb:.1} MB ({:.0}%)", self.ratio() * 100.0)
            }
            None => format!("{downloaded_mb:.1} MB downloaded"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: Version,
    pub download_url: String,
    pub asset_name: String,
    pub asset_size: u64,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn current_version() -> Version {
    Version::parse(VERSION).expect("invalid CARGO_PKG_VERSION")
}

fn parse_tag_version(tag: &str) -> Result<Version, String> {
    let trimmed = tag.trim().trim_start_matches('v');
    Version::parse(trimmed).map_err(|e| format!("invalid release tag {tag:?}: {e}"))
}

fn platform_asset_name() -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    {
        return Some("shocking_vrc_cli.exe");
    }
    #[cfg(target_os = "linux")]
    {
        return Some("shocking_vrc_cli");
    }
    #[cfg(target_os = "macos")]
    {
        return Some("shocking_vrc_cli-macos");
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("ShockingVRC/{VERSION}"))
        .build()
        .map_err(|e| e.to_string())
}

pub fn cleanup_staging_files() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "shocking_vrc_cli".into());

    let leftovers = [
        dir.join(format!("{stem}.old")),
        dir.join(format!("{stem}.update")),
        dir.join("shockingvrc_update.bat"),
    ];
    for path in leftovers {
        let _ = std::fs::remove_file(path);
    }
}

pub async fn check_for_update() -> Result<Option<ReleaseInfo>, String> {
    let asset_name = platform_asset_name().ok_or("unsupported platform for auto-update")?;
    let client = http_client()?;
    let resp = client
        .get(API_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("release check failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("release check HTTP {}", resp.status()));
    }

    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("invalid release JSON: {e}"))?;

    let remote = parse_tag_version(&release.tag_name)?;
    let local = current_version();
    if remote <= local {
        return Ok(None);
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| format!("release {} has no asset {asset_name}", release.tag_name))?;

    Ok(Some(ReleaseInfo {
        tag: release.tag_name,
        version: remote,
        download_url: asset.browser_download_url.clone(),
        asset_name: asset.name.clone(),
        asset_size: asset.size,
    }))
}

pub async fn download_release(
    release: &ReleaseInfo,
    mut on_progress: impl FnMut(DownloadProgress) + Send,
) -> Result<PathBuf, String> {
    let client = http_client()?;
    let resp = client
        .get(&release.download_url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("download HTTP {}", resp.status()));
    }

    let total = resp
        .content_length()
        .filter(|n| *n > 0)
        .or((release.asset_size > 0).then_some(release.asset_size));

    let tmp = std::env::temp_dir().join(format!(
        "shocking_vrc_cli_{}_{}",
        release.tag.replace('/', "_"),
        release.asset_name
    ));

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;

    let mut downloaded = 0u64;
    on_progress(DownloadProgress { downloaded, total });

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download read failed: {e}"))?;
        downloaded += chunk.len() as u64;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        on_progress(DownloadProgress { downloaded, total });
    }

    file.flush()
        .await
        .map_err(|e| format!("flush {}: {e}", tmp.display()))?;

    Ok(tmp)
}

pub fn apply_self_update(downloaded: &Path, restart_args: &[String]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        return apply_self_update_windows(downloaded, restart_args);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (downloaded, restart_args);
        Err("auto-install is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn apply_self_update_windows(downloaded: &Path, restart_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current executable has no parent directory".to_string())?;

    let file_name = exe
        .file_name()
        .ok_or_else(|| "current executable has no file name".to_string())?;
    let staging = dir.join(format!("{}.update", file_name.to_string_lossy()));
    std::fs::copy(downloaded, &staging).map_err(|e| format!("stage update: {e}"))?;

    let batch_path = dir.join("shockingvrc_update.bat");
    let pid = std::process::id();
    let args_line = restart_args
        .iter()
        .map(|arg| {
            let escaped = arg.replace('"', "");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let script = format!(
        r#"@echo off
setlocal
set "PID={pid}"
set "TARGET={exe}"
set "STAGING={staging}"
:wait_pid
tasklist /FI "PID eq %PID%" 2>nul | find " %PID% " >nul
if not errorlevel 1 (
  ping 127.0.0.1 -n 2 >nul
  goto wait_pid
)
ping 127.0.0.1 -n 3 >nul
:wait_unlock
del /f /q "%TARGET%" >nul 2>&1
if exist "%TARGET%" (
  ping 127.0.0.1 -n 2 >nul
  goto wait_unlock
)
move /y "%STAGING%" "%TARGET%" >nul
start "" "%TARGET%" {args}
del /f /q "%~f0" >nul 2>&1
"#,
        pid = pid,
        exe = exe.display(),
        staging = staging.display(),
        args = args_line,
    );
    std::fs::write(&batch_path, script).map_err(|e| format!("write updater script: {e}"))?;

    std::process::Command::new("cmd")
        .args([
            "/C",
            "start",
            "/MIN",
            "",
            &batch_path.to_string_lossy(),
        ])
        .spawn()
        .map_err(|e| format!("launch updater: {e}"))?;

    Ok(())
}

pub fn release_page_url() -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_tags() {
        assert_eq!(parse_tag_version("1.0.1").unwrap(), Version::new(1, 0, 1));
        assert_eq!(parse_tag_version("v2.3.4").unwrap(), Version::new(2, 3, 4));
    }
}
