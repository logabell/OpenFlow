use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use tar::Archive;
use zip::read::ZipArchive;

use super::{
    manager::{ArchiveFormat, ModelAsset},
    metadata::compute_sha256,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPlan {
    pub uri: String,
    pub archive_format: ArchiveFormat,
    pub destination: PathBuf,
    pub strip_prefix_components: u8,
    pub expected_size_bytes: Option<u64>,
    pub expected_checksum: Option<String>,
    pub filename: Option<String>,
}

impl DownloadPlan {
    #[must_use]
    pub fn staging_path(&self) -> PathBuf {
        let mut path = self.destination.clone();
        let ext = format!("download.{}", self.archive_format.extension());
        path.set_extension(ext);
        path
    }
}

pub fn plan_for(asset: &ModelAsset, models_dir: PathBuf) -> Option<DownloadPlan> {
    let source = asset.source.as_ref()?;
    Some(DownloadPlan {
        uri: source.uri.clone(),
        archive_format: source.archive_format,
        destination: asset.path(&models_dir),
        strip_prefix_components: source.strip_prefix_components,
        expected_size_bytes: if asset.size_bytes > 0 {
            Some(asset.size_bytes)
        } else {
            None
        },
        expected_checksum: asset.checksum.clone(),
        filename: filename_from_uri(&source.uri),
    })
}

#[derive(Debug)]
pub struct DownloadOutcome {
    pub final_path: PathBuf,
    pub archive_size_bytes: u64,
    pub checksum: String,
}

pub fn download_and_extract_with_progress<F>(
    plan: &DownloadPlan,
    mut progress: F,
) -> Result<DownloadOutcome>
where
    F: FnMut(DownloadProgress),
{
    let client = Client::builder().build().context("create http client")?;
    let staging = plan.staging_path();
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent).context("create staging directory")?;
    }

    let _bytes_downloaded = download_to_file(&client, plan, &staging, &mut progress)?;

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
        archive_size_bytes: size,
        checksum,
    })
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

fn download_to_file<F>(
    client: &Client,
    plan: &DownloadPlan,
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

fn extract_archive(plan: &DownloadPlan, archive_path: &Path) -> Result<()> {
    let file = File::open(archive_path).context("open archive")?;
    match plan.archive_format {
        ArchiveFormat::TarGz => extract_tar(plan, GzDecoder::new(file)),
        ArchiveFormat::TarBz2 => extract_tar(plan, BzDecoder::new(file)),
        ArchiveFormat::Zip => extract_zip(plan, file),
        ArchiveFormat::File => extract_file(plan, file, archive_path),
    }
}

fn extract_tar<R: Read>(plan: &DownloadPlan, reader: R) -> Result<()> {
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

fn extract_zip(plan: &DownloadPlan, file: File) -> Result<()> {
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

fn extract_file(plan: &DownloadPlan, mut file: File, archive_path: &Path) -> Result<()> {
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
