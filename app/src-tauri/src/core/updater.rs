use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/logabell/OpenFlow/releases/latest/download/latest.json";

fn env_flag_enabled(key: &str) -> bool {
    let value = match std::env::var(key) {
        Ok(value) => value,
        Err(_) => return false,
    };

    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

fn disable_update_checks() -> bool {
    env_flag_enabled("OPENFLOW_TEST_MODE") || env_flag_enabled("OPENFLOW_DISABLE_UPDATE_CHECK")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatestAsset {
    tarball: String,
    sha256_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatestManifest {
    version: String,
    assets: std::collections::HashMap<String, LatestAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCache {
    checked_at_unix: i64,
    manifest: LatestManifest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tarball_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_url: Option<String>,
    pub checked_at_unix: i64,
    pub from_cache: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedUpdate {
    pub version: String,
    pub tarball_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    pub stage: String,
    pub downloaded_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplyProgress {
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "OpenFlow", "OpenFlow").context("missing project directories")
}

fn cache_file() -> Result<PathBuf> {
    Ok(project_dirs()?.cache_dir().join("update-cache.json"))
}

fn updates_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.cache_dir().join("updates"))
}

fn manifest_url() -> String {
    std::env::var("OPENFLOW_UPDATE_MANIFEST_URL").unwrap_or_else(|_| DEFAULT_MANIFEST_URL.into())
}

fn build_flavor_from_install_dir() -> Option<String> {
    let override_key = std::env::var("OPENFLOW_UPDATE_ASSET_KEY").ok();
    if let Some(value) = override_key {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let path = dir.join("BUILD_FLAVOR");
    let contents = fs::read_to_string(path).ok()?;
    let first = contents.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

fn infer_asset_key_from_exe_binary() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = fs::read(exe).ok()?;

    // This is a pragmatic heuristic: the binary typically contains the SONAME it links against.
    // If this ever fails, builds should still include BUILD_FLAVOR (preferred path).
    if bytes
        .windows(b"libwebkit2gtk-4.1.so.0".len())
        .any(|w| w == b"libwebkit2gtk-4.1.so.0")
    {
        return Some("linux-x86_64-webkit41".to_string());
    }

    if bytes
        .windows(b"libwebkit2gtk-4.0.so".len())
        .any(|w| w == b"libwebkit2gtk-4.0.so")
    {
        return Some("linux-x86_64-webkit40".to_string());
    }

    None
}

fn select_asset_key(manifest: &LatestManifest) -> Result<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(key) = build_flavor_from_install_dir() {
        candidates.push(key);
    }
    if let Some(key) = infer_asset_key_from_exe_binary() {
        if !candidates.contains(&key) {
            candidates.push(key);
        }
    }

    // Sensible fallbacks.
    for key in [
        "linux-x86_64-webkit41",
        "linux-x86_64-webkit40",
        "linux-x86_64",
    ] {
        let key = key.to_string();
        if !candidates.contains(&key) {
            candidates.push(key);
        }
    }

    for key in &candidates {
        if manifest.assets.contains_key(key) {
            return Ok(key.clone());
        }
    }

    if manifest.assets.len() == 1 {
        if let Some((key, _)) = manifest.assets.iter().next() {
            return Ok(key.clone());
        }
    }

    let available: Vec<String> = manifest.assets.keys().cloned().collect();
    Err(anyhow!(
        "latest.json missing a compatible asset. Tried {:?}. Available: {:?}",
        candidates,
        available
    ))
}

fn base_url_from_manifest_url(url: &str) -> Result<String> {
    let trimmed = url
        .strip_suffix("/latest.json")
        .or_else(|| url.strip_suffix("latest.json"))
        .ok_or_else(|| anyhow!("manifest url must end with latest.json"))?;
    Ok(trimmed.to_string())
}

fn parse_version_triplet(input: &str) -> Option<(u64, u64, u64)> {
    let trimmed = input.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (
        parse_version_triplet(latest),
        parse_version_triplet(current),
    ) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

fn read_cache(path: &Path) -> Option<UpdateCache> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_cache(path: &Path, cache: &UpdateCache) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(cache) {
        let _ = fs::write(path, bytes);
    }
}

fn fetch_manifest(client: &Client, url: &str) -> Result<LatestManifest> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?;
    response
        .json::<LatestManifest>()
        .context("parse latest.json manifest")
}

pub fn check_for_updates(force: bool) -> Result<UpdateCheckResult> {
    let current_version = format!("v{}", env!("CARGO_PKG_VERSION"));

    if disable_update_checks() {
        let checked_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        return Ok(UpdateCheckResult {
            current_version: current_version.clone(),
            latest_version: current_version,
            update_available: false,
            tarball_url: None,
            sha256_url: None,
            checked_at_unix,
            from_cache: false,
        });
    }

    let url = manifest_url();
    let base_url = base_url_from_manifest_url(&url)?;

    let now = OffsetDateTime::now_utc();
    let cache_path = cache_file()?;

    let max_age = Duration::hours(24);
    if !force {
        if let Some(cache) = read_cache(&cache_path) {
            let checked_at = OffsetDateTime::from_unix_timestamp(cache.checked_at_unix).ok();
            if let Some(checked_at) = checked_at {
                if now - checked_at < max_age {
                    return build_result(
                        &current_version,
                        &base_url,
                        cache.manifest,
                        cache.checked_at_unix,
                        true,
                    );
                }
            }
        }
    }

    let client = Client::builder().build().context("create http client")?;
    let manifest = fetch_manifest(&client, &url)?;
    let checked_at_unix = now.unix_timestamp();
    write_cache(
        &cache_path,
        &UpdateCache {
            checked_at_unix,
            manifest: manifest.clone(),
        },
    );

    build_result(
        &current_version,
        &base_url,
        manifest,
        checked_at_unix,
        false,
    )
}

fn build_result(
    current_version: &str,
    base_url: &str,
    manifest: LatestManifest,
    checked_at_unix: i64,
    from_cache: bool,
) -> Result<UpdateCheckResult> {
    let latest_version = manifest.version.clone();
    let update_available = is_newer(&latest_version, current_version);

    let asset_key = select_asset_key(&manifest)?;
    let asset = manifest
        .assets
        .get(&asset_key)
        .cloned()
        .ok_or_else(|| anyhow!("latest.json missing assets.{asset_key}"))?;

    let tarball_url = format!("{}/{}", base_url.trim_end_matches('/'), asset.tarball);
    let sha256_url = format!("{}/{}", base_url.trim_end_matches('/'), asset.sha256_file);

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version,
        update_available,
        tarball_url: Some(tarball_url),
        sha256_url: Some(sha256_url),
        checked_at_unix,
        from_cache,
    })
}

#[allow(dead_code)]
pub fn download_update(force: bool) -> Result<DownloadedUpdate> {
    download_update_with_progress(force, |_| {})
}

pub fn download_update_with_progress<F>(force: bool, mut on_progress: F) -> Result<DownloadedUpdate>
where
    F: FnMut(UpdateDownloadProgress),
{
    let info = check_for_updates(force)?;
    if !info.update_available {
        return Ok(DownloadedUpdate {
            version: info.latest_version,
            tarball_path: String::new(),
        });
    }

    let tarball_url = info
        .tarball_url
        .clone()
        .ok_or_else(|| anyhow!("missing tarball url"))?;
    let sha_url = info
        .sha256_url
        .clone()
        .ok_or_else(|| anyhow!("missing sha256 url"))?;

    let dir = updates_dir()?;
    fs::create_dir_all(&dir).context("create updates directory")?;

    let tarball_path = dir.join("openflow-update.tar.gz");
    let sha_path = dir.join("openflow-update.tar.gz.sha256");

    if !force && tarball_path.is_file() && sha_path.is_file() {
        if verify_sha256_file(&tarball_path, &sha_path).is_ok() {
            return Ok(DownloadedUpdate {
                version: info.latest_version,
                tarball_path: tarball_path.display().to_string(),
            });
        }
    }

    let client = Client::builder().build().context("create http client")?;

    download_url_to_file_with_progress(&client, &tarball_url, &tarball_path, |d, t| {
        on_progress(UpdateDownloadProgress {
            stage: "tarball".to_string(),
            downloaded_bytes: d,
            total_bytes: t,
        });
    })?;

    download_url_to_file_with_progress(&client, &sha_url, &sha_path, |d, t| {
        on_progress(UpdateDownloadProgress {
            stage: "sha256".to_string(),
            downloaded_bytes: d,
            total_bytes: t,
        });
    })?;

    verify_sha256_file(&tarball_path, &sha_path)?;

    Ok(DownloadedUpdate {
        version: info.latest_version,
        tarball_path: tarball_path.display().to_string(),
    })
}

fn download_url_to_file_with_progress(
    client: &Client,
    url: &str,
    path: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("download {url}"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create download parent")?;
    }

    let mut file = fs::File::create(path).context("create download file")?;
    let mut buffer = [0u8; 32 * 1024];

    let total = response.content_length();
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut last_bytes = 0u64;

    on_progress(downloaded, total);
    loop {
        let read = response.read(&mut buffer).context("read download chunk")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .context("write download chunk")?;

        downloaded = downloaded.saturating_add(read as u64);
        let now = std::time::Instant::now();
        let should_emit = now.duration_since(last_emit) >= std::time::Duration::from_millis(125)
            || downloaded.saturating_sub(last_bytes) >= 256 * 1024
            || total.is_some_and(|t| downloaded >= t);
        if should_emit {
            last_emit = now;
            last_bytes = downloaded;
            on_progress(downloaded, total);
        }
    }

    on_progress(downloaded, total);
    Ok(())
}

fn verify_sha256_file(tarball: &Path, sha_file: &Path) -> Result<()> {
    let expected = fs::read_to_string(sha_file)
        .with_context(|| format!("read sha256 file {}", sha_file.display()))?
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("sha256 file missing hash"))?
        .to_string();

    let actual = crate::models::compute_sha256(tarball)?;
    if actual != expected {
        anyhow::bail!("sha256 mismatch: expected {} got {}", expected, actual);
    }
    Ok(())
}

#[allow(dead_code)]
pub fn apply_update_with_pkexec(tarball_path: &str) -> Result<()> {
    apply_update_with_pkexec_with_progress(tarball_path, |_| {})
}

pub fn apply_update_with_pkexec_with_progress<F>(
    tarball_path: &str,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(UpdateApplyProgress),
{
    if !Path::new(tarball_path).exists() {
        anyhow::bail!("update tarball not found: {tarball_path}");
    }

    let allowed_dir = updates_dir()?;
    let tarball = PathBuf::from(tarball_path);
    let canonical = tarball
        .canonicalize()
        .with_context(|| format!("canonicalize {tarball_path}"))?;
    let allowed = allowed_dir.canonicalize().unwrap_or(allowed_dir);
    if !canonical.starts_with(&allowed) {
        anyhow::bail!("refusing to apply update from outside cache dir");
    }

    let pkexec = if Path::new("/usr/bin/pkexec").is_file() {
        "/usr/bin/pkexec"
    } else {
        "pkexec"
    };

    let script = r#"set -eu

TARBALL="$1"
INSTALL_DIR="/opt/openflow"

progress() {
  echo "OPENFLOW_APPLY_PROGRESS:$1"
}

progress "starting"

STAGE="$(mktemp -d)"
cleanup() {
  rm -rf "$STAGE"
}
trap cleanup EXIT

progress "extract"
mkdir -p "$STAGE/extract"
tar -xzf "$TARBALL" -C "$STAGE/extract"

progress "validate"
if [ ! -x "$STAGE/extract/openflow/openflow" ]; then
  echo "invalid update payload (missing openflow launcher)" >&2
  exit 1
fi
if [ ! -x "$STAGE/extract/openflow/openflow-bin" ]; then
  echo "invalid update payload (missing openflow binary)" >&2
  exit 1
fi
if [ ! -d "$STAGE/extract/openflow/lib" ]; then
  echo "invalid update payload (missing lib directory)" >&2
  exit 1
fi
if [ ! -f "$STAGE/extract/openflow/lib/libsherpa-onnx-c-api.so" ]; then
  echo "invalid update payload (missing libsherpa-onnx-c-api.so)" >&2
  exit 1
fi
if [ ! -f "$STAGE/extract/openflow/lib/libsherpa-onnx-cxx-api.so" ]; then
  echo "invalid update payload (missing libsherpa-onnx-cxx-api.so)" >&2
  exit 1
fi

progress "swap"
rm -rf "$INSTALL_DIR.new"
rm -rf "$INSTALL_DIR.old"

mv "$STAGE/extract/openflow" "$INSTALL_DIR.new"
if [ -d "$INSTALL_DIR" ]; then
  mv "$INSTALL_DIR" "$INSTALL_DIR.old"
fi
mv "$INSTALL_DIR.new" "$INSTALL_DIR"
rm -rf "$INSTALL_DIR.old"

progress "permissions"
chown -R root:root "$INSTALL_DIR"
chmod 0755 "$INSTALL_DIR/openflow" "$INSTALL_DIR/openflow-bin"

progress "done"
"#;

    on_progress(UpdateApplyProgress {
        stage: "auth".to_string(),
        message: Some("Waiting for admin approval".to_string()),
    });

    let mut child = std::process::Command::new(pkexec)
        .arg("sh")
        .arg("-c")
        .arg(script)
        .arg("_")
        .arg(canonical)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawn pkexec")?;

    let stdout = child.stdout.take().context("capture pkexec stdout")?;
    let stderr = child.stderr.take().context("capture pkexec stderr")?;

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        let _ = reader.read_to_string(&mut buf);
        buf
    });

    for line in BufReader::new(stdout).lines() {
        let line = line.unwrap_or_default();
        let Some(stage) = line.strip_prefix("OPENFLOW_APPLY_PROGRESS:") else {
            continue;
        };
        let stage = stage.trim();
        if stage.is_empty() {
            continue;
        }
        on_progress(UpdateApplyProgress {
            stage: stage.to_string(),
            message: None,
        });
    }

    let status = child.wait().context("wait for pkexec")?;
    let stderr_text = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        let stderr_trimmed = stderr_text.trim();
        if stderr_trimmed.is_empty() {
            anyhow::bail!("pkexec failed with status {status}");
        }
        anyhow::bail!("pkexec failed with status {status}: {stderr_trimmed}");
    }

    on_progress(UpdateApplyProgress {
        stage: "done".to_string(),
        message: None,
    });

    Ok(())
}
