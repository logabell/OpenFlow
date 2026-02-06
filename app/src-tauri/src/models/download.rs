use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use globset::{Glob, GlobSet, GlobSetBuilder};
use reqwest::blocking::Client;
use serde::Deserialize;
use tar::Archive;
use zip::read::ZipArchive;

use super::{
    manager::{ArchiveFormat, ModelArchiveSource, ModelAsset, ModelHfSource, ModelSource},
    metadata::compute_sha256,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDownloadPlan {
    pub uri: String,
    pub archive_format: ArchiveFormat,
    pub destination: PathBuf,
    pub strip_prefix_components: u8,
    pub expected_size_bytes: Option<u64>,
    pub expected_checksum: Option<String>,
    pub filename: Option<String>,
}

impl ArchiveDownloadPlan {
    #[must_use]
    pub fn staging_path(&self) -> PathBuf {
        let mut path = self.destination.clone();
        let ext = format!("download.{}", self.archive_format.extension());
        path.set_extension(ext);
        path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRepoDownloadPlan {
    pub repo: String,
    pub revision: String,
    pub destination: PathBuf,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadPlan {
    Archive(ArchiveDownloadPlan),
    HfRepo(HfRepoDownloadPlan),
}

pub fn plan_for(asset: &ModelAsset, models_dir: PathBuf) -> Option<DownloadPlan> {
    let source = asset.source.as_ref()?;
    match source {
        ModelSource::Archive(ModelArchiveSource {
            uri,
            archive_format,
            strip_prefix_components,
        }) => Some(DownloadPlan::Archive(ArchiveDownloadPlan {
            uri: uri.clone(),
            archive_format: *archive_format,
            destination: asset.path(&models_dir),
            strip_prefix_components: *strip_prefix_components,
            expected_size_bytes: if asset.size_bytes > 0 {
                Some(asset.size_bytes)
            } else {
                None
            },
            expected_checksum: asset.checksum.clone(),
            filename: filename_from_uri(uri),
        })),
        ModelSource::HfRepo(ModelHfSource {
            repo,
            revision,
            include,
            exclude,
        }) => Some(DownloadPlan::HfRepo(HfRepoDownloadPlan {
            repo: repo.clone(),
            revision: revision.clone().unwrap_or_else(|| "main".into()),
            destination: asset.path(&models_dir),
            include: include.clone(),
            exclude: exclude.clone(),
        })),
    }
}

#[derive(Debug)]
pub struct DownloadOutcome {
    pub final_path: PathBuf,
    pub total_size_bytes: u64,
    pub checksum: Option<String>,
}

pub fn download_and_extract_with_progress<F>(
    plan: &DownloadPlan,
    mut progress: F,
) -> Result<DownloadOutcome>
where
    F: FnMut(DownloadProgress),
{
    let client = Client::builder().build().context("create http client")?;
    match plan {
        DownloadPlan::Archive(plan) => download_archive(&client, plan, &mut progress),
        DownloadPlan::HfRepo(plan) => download_hf_repo(&client, plan, &mut progress),
    }
}

impl ArchiveFormat {
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarBz2 => "tar.bz2",
            ArchiveFormat::File => "bin",
        }
    }
}

pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

fn download_archive<F>(
    client: &Client,
    plan: &ArchiveDownloadPlan,
    progress: &mut F,
) -> Result<DownloadOutcome>
where
    F: FnMut(DownloadProgress),
{
    let staging = plan.staging_path();
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent).context("create staging directory")?;
    }

    let _bytes_downloaded = download_to_file(client, plan, &staging, progress)?;

    let size = fs::metadata(&staging)
        .context("stat downloaded file")?
        .len();
    if let Some(expected) = plan.expected_size_bytes {
        if size != expected {
            return Err(anyhow!(
                "size mismatch: expected {} bytes, got {}",
                expected,
                size
            ));
        }
    }

    let checksum = compute_sha256(&staging)?;
    if let Some(expected) = &plan.expected_checksum {
        if &checksum != expected {
            return Err(anyhow!(
                "checksum mismatch: expected {}, got {}",
                expected,
                checksum
            ));
        }
    }

    if plan.destination.exists() {
        fs::remove_dir_all(&plan.destination).with_context(|| {
            format!("remove existing destination {}", plan.destination.display())
        })?;
    }
    fs::create_dir_all(&plan.destination).context("create destination directory")?;

    extract_archive(plan, &staging)?;

    let _ = fs::remove_file(&staging);

    Ok(DownloadOutcome {
        final_path: plan.destination.clone(),
        total_size_bytes: size,
        checksum: Some(checksum),
    })
}

fn download_hf_repo<F>(
    client: &Client,
    plan: &HfRepoDownloadPlan,
    progress: &mut F,
) -> Result<DownloadOutcome>
where
    F: FnMut(DownloadProgress),
{
    let files = list_hf_repo_files(client, plan)?;
    if files.is_empty() {
        return Err(anyhow!("no downloadable files found in HF repo"));
    }

    let total_size = files.iter().map(|file| file.size.unwrap_or(0)).sum();
    let total = if total_size > 0 {
        Some(total_size)
    } else {
        None
    };

    let staging = plan.destination.with_extension("download");
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging).context("create hf staging directory")?;

    let mut downloaded = 0u64;
    for file in files {
        let target = staging.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).context("create hf file parent")?;
        }
        downloaded += download_hf_file(client, &file.uri, &target, downloaded, total, progress)?;
    }

    if plan.destination.exists() {
        fs::remove_dir_all(&plan.destination).with_context(|| {
            format!("remove existing destination {}", plan.destination.display())
        })?;
    }
    fs::rename(&staging, &plan.destination).context("finalize hf download")?;

    Ok(DownloadOutcome {
        final_path: plan.destination.clone(),
        total_size_bytes: total_size,
        checksum: None,
    })
}

fn download_to_file<F>(
    client: &Client,
    plan: &ArchiveDownloadPlan,
    path: &Path,
    progress: &mut F,
) -> Result<u64>
where
    F: FnMut(DownloadProgress),
{
    let response = client
        .get(&plan.uri)
        .send()
        .with_context(|| format!("request {}", plan.uri))?
        .error_for_status()
        .with_context(|| format!("download {}", plan.uri))?;

    let content_length = response.content_length();
    let total = plan.expected_size_bytes.or(content_length);
    let mut response = response;

    let mut file = File::create(path).context("create staging file")?;
    let mut downloaded = 0u64;
    const CHUNK_SIZE: usize = 32 * 1024;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        let read = response.read(&mut buffer).context("read download chunk")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .context("write download chunk")?;
        downloaded += read as u64;
        progress(DownloadProgress { downloaded, total });
    }
    Ok(downloaded)
}

fn download_hf_file<F>(
    client: &Client,
    uri: &str,
    path: &Path,
    start_offset: u64,
    total: Option<u64>,
    progress: &mut F,
) -> Result<u64>
where
    F: FnMut(DownloadProgress),
{
    let response = client
        .get(uri)
        .send()
        .with_context(|| format!("request {}", uri))?
        .error_for_status()
        .with_context(|| format!("download {}", uri))?;

    let mut response = response;
    let mut file = File::create(path).context("create hf file")?;
    let mut downloaded = 0u64;
    const CHUNK_SIZE: usize = 32 * 1024;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        let read = response.read(&mut buffer).context("read hf chunk")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).context("write hf chunk")?;
        downloaded += read as u64;
        progress(DownloadProgress {
            downloaded: start_offset + downloaded,
            total,
        });
    }
    Ok(downloaded)
}

fn extract_archive(plan: &ArchiveDownloadPlan, archive_path: &Path) -> Result<()> {
    let file = File::open(archive_path).context("open archive")?;
    match plan.archive_format {
        ArchiveFormat::TarGz => extract_tar(plan, GzDecoder::new(file)),
        ArchiveFormat::TarBz2 => extract_tar(plan, BzDecoder::new(file)),
        ArchiveFormat::Zip => extract_zip(plan, file),
        ArchiveFormat::File => extract_file(plan, file, archive_path),
    }
}

fn extract_tar<R: Read>(plan: &ArchiveDownloadPlan, reader: R) -> Result<()> {
    let mut archive = Archive::new(reader);
    for entry in archive.entries().context("iterate tar entries")? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("read entry path")?.into_owned();
        let relative = strip_components(&path, plan.strip_prefix_components).ok_or_else(|| {
            anyhow!(
                "unable to strip {} components from {:?}",
                plan.strip_prefix_components,
                path
            )
        })?;
        let dest = if relative.as_os_str() == "." {
            plan.destination.clone()
        } else {
            plan.destination.join(relative)
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).context("create entry parent")?;
        }
        entry.unpack(&dest).context("unpack tar entry")?;
    }
    Ok(())
}

fn extract_zip(plan: &ArchiveDownloadPlan, file: File) -> Result<()> {
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("read zip entry")?;
        let path = entry.mangled_name();
        let relative = strip_components(&path, plan.strip_prefix_components).ok_or_else(|| {
            anyhow!(
                "unable to strip {} components from {:?}",
                plan.strip_prefix_components,
                path
            )
        })?;
        let dest = if relative.as_os_str() == "." {
            plan.destination.clone()
        } else {
            plan.destination.join(relative)
        };
        if entry.is_dir() {
            fs::create_dir_all(&dest).context("create zip dir")?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).context("create zip file parent")?;
            }
            let mut outfile = File::create(&dest).context("create zip file")?;
            io::copy(&mut entry, &mut outfile).context("write zip file")?;
        }
    }
    Ok(())
}

fn extract_file(plan: &ArchiveDownloadPlan, mut file: File, archive_path: &Path) -> Result<()> {
    let filename = plan
        .filename
        .as_ref()
        .map(|name| PathBuf::from(name))
        .or_else(|| archive_path.file_name().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("model.bin"));
    let target = plan.destination.join(filename);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).context("create file parent")?;
    }
    let mut dest = File::create(&target).context("create target file")?;
    io::copy(&mut file, &mut dest).context("copy plain file")?;
    Ok(())
}

fn filename_from_uri(uri: &str) -> Option<String> {
    let last_segment = uri.split('/').last()?;
    let clean = last_segment.split('?').next()?.split('#').next()?.trim();
    if clean.is_empty() {
        None
    } else {
        Some(clean.to_string())
    }
}

fn strip_components(path: &Path, count: u8) -> Option<PathBuf> {
    let mut components = path.components();
    for _ in 0..count {
        components.next()?;
    }
    let stripped: PathBuf = components.collect();
    Some(if stripped.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        stripped
    })
}

#[derive(Debug, Deserialize)]
struct HfModelInfo {
    #[serde(default)]
    siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug)]
struct HfRepoFile {
    path: String,
    uri: String,
    size: Option<u64>,
}

fn list_hf_repo_files(client: &Client, plan: &HfRepoDownloadPlan) -> Result<Vec<HfRepoFile>> {
    let info_url = format!("https://huggingface.co/api/models/{}", plan.repo);
    let info: HfModelInfo = client
        .get(&info_url)
        .send()
        .with_context(|| format!("request {info_url}"))?
        .error_for_status()
        .with_context(|| format!("fetch hf model metadata for {}", plan.repo))?
        .json()
        .context("parse hf metadata")?;

    let include = compile_globset(&plan.include)?;
    let exclude = compile_globset(&plan.exclude)?;

    let mut files = Vec::new();
    for sibling in info.siblings {
        let filename = sibling.rfilename.replace('\\', "/");
        if let Some(include) = &include {
            if !include.is_match(&filename) {
                continue;
            }
        }
        if let Some(exclude) = &exclude {
            if exclude.is_match(&filename) {
                continue;
            }
        }
        let uri = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            plan.repo, plan.revision, filename
        );
        files.push(HfRepoFile {
            path: filename,
            uri,
            size: sibling.size,
        });
    }

    Ok(files)
}

fn compile_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob =
            Glob::new(pattern).with_context(|| format!("invalid glob pattern: {}", pattern))?;
        builder.add(glob);
    }
    let set = builder.build().context("build globset")?;
    Ok(Some(set))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn glob_double_star_matches_root_and_nested() {
        let set = compile_globset(&["**/*.json".to_string()])
            .unwrap()
            .unwrap();
        assert!(set.is_match("config.json"));
        assert!(set.is_match("subdir/config.json"));
    }

    #[test]
    fn glob_excludes_apply() {
        let include = compile_globset(&["**/*.onnx".to_string()])
            .unwrap()
            .unwrap();
        let exclude = compile_globset(&["**/*.int8.onnx".to_string()])
            .unwrap()
            .unwrap();
        assert!(include.is_match("model.onnx"));
        assert!(include.is_match("model.int8.onnx"));
        assert!(!exclude.is_match("model.onnx"));
        assert!(exclude.is_match("model.int8.onnx"));
    }

    // Metadata-only smoke test against HuggingFace API.
    // Keeps assertions minimal to reduce flake.
    #[test]
    fn hf_metadata_filters_non_empty_for_known_repos() {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("client");

        // Faster-Whisper CT2 (should have root-level files like config/tokenizer json).
        let ct2_plan = HfRepoDownloadPlan {
            repo: "Systran/faster-whisper-tiny".into(),
            revision: "main".into(),
            destination: PathBuf::from("/tmp/unused"),
            include: vec!["**/*.bin".into(), "**/*.json".into(), "**/*.txt".into()],
            exclude: Vec::new(),
        };
        let ct2_files = list_hf_repo_files(&client, &ct2_plan).expect("ct2 list");
        assert!(!ct2_files.is_empty(), "ct2 filter returned no files");

        // Sherpa ONNX whisper float plan should exclude int8 models.
        let onnx_plan = HfRepoDownloadPlan {
            repo: "csukuangfj/sherpa-onnx-whisper-tiny".into(),
            revision: "main".into(),
            destination: PathBuf::from("/tmp/unused"),
            include: vec![
                "**/*.onnx".into(),
                "**/*.weights".into(),
                "**/*.txt".into(),
                "**/*.json".into(),
            ],
            exclude: vec!["**/*.int8.onnx".into()],
        };
        let onnx_files = list_hf_repo_files(&client, &onnx_plan).expect("onnx list");
        assert!(!onnx_files.is_empty(), "onnx filter returned no files");
        assert!(
            !onnx_files.iter().any(|f| f.path.ends_with(".int8.onnx")),
            "exclude glob did not exclude .int8.onnx files"
        );
    }
}
