use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{unbounded, Receiver, Sender};
use tauri::{AppHandle, Manager};

use crate::core::{app_state::AppState, events};

use super::{
    build_download_plan, download_and_extract_with_progress, DownloadOutcome, DownloadProgress,
    ModelAsset, ModelKind, ModelManager, ModelStatus,
};

use super::metadata::total_size;

#[derive(Debug, Clone)]
pub struct ModelDownloadJob {
    pub asset_name: String,
}

#[derive(Debug)]
pub struct ModelDownloadService {
    sender: Sender<ModelDownloadJob>,
}

impl Clone for ModelDownloadService {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl ModelDownloadService {
    pub fn new(app: AppHandle, manager: Arc<Mutex<ModelManager>>) -> Result<Self> {
        let (sender, receiver) = unbounded();
        let models_dir = {
            let guard = manager.lock().map_err(|err| anyhow!(err.to_string()))?;
            guard.root().to_path_buf()
        };
        thread::spawn(move || worker_loop(receiver, manager, models_dir, app));
        Ok(Self { sender })
    }

    pub fn queue(&self, job: ModelDownloadJob) -> Result<()> {
        self.sender
            .send(job)
            .context("send model download job to worker")
    }
}

fn worker_loop(
    receiver: Receiver<ModelDownloadJob>,
    manager: Arc<Mutex<ModelManager>>,
    models_dir: PathBuf,
    app: AppHandle,
) {
    for job in receiver.iter() {
        let mut initial_events: Vec<ModelAsset> = Vec::new();
        let selection_plan = {
            let mut guard = match manager.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };

            let result = guard.assets_mut().into_iter().find_map(|asset| {
                if asset.name != job.asset_name {
                    return None;
                }

                if !matches!(
                    asset.status,
                    ModelStatus::NotInstalled | ModelStatus::Error(_)
                ) {
                    return None;
                }

                if asset.source.is_none() {
                    asset.status = ModelStatus::Error("missing download source".into());
                    initial_events.push(asset.clone());
                    return Some((asset.name.clone(), None));
                }

                asset.status = ModelStatus::Downloading {
                    progress: 0.0,
                    downloaded_bytes: 0,
                    total_bytes: None,
                };
                let name = asset.name.clone();
                let plan = build_download_plan(asset, models_dir.clone());
                initial_events.push(asset.clone());
                Some((name, plan))
            });

            let _ = guard.save();
            drop(guard);

            result
        };
        for snapshot in initial_events {
            emit_status(&app, snapshot);
        }

        let Some((asset_name, plan)) = selection_plan else {
            continue;
        };

        let Some(plan) = plan else {
            continue;
        };

        let mut last_emit_at = Instant::now() - Duration::from_secs(5);
        let mut last_progress_bucket: i32 = -1;

        match download_and_extract_with_progress(&plan, |progress: DownloadProgress| {
            let fraction = progress_fraction(progress.downloaded, progress.total);
            let bucket = (fraction * 100.0).floor() as i32;
            let now = Instant::now();
            let should_emit = now.duration_since(last_emit_at) >= Duration::from_millis(150)
                || bucket >= last_progress_bucket + 1
                || progress
                    .total
                    .is_some_and(|t| t > 0 && progress.downloaded >= t);

            if !should_emit {
                return;
            }
            last_emit_at = now;
            last_progress_bucket = bucket;

            on_progress(
                &manager,
                &app,
                &asset_name,
                progress.downloaded,
                progress.total,
            );
        }) {
            Ok(outcome) => on_download_success(&manager, &app, &asset_name, &outcome),
            Err(error) => on_download_failure(&manager, &app, &asset_name, error),
        }
    }
}

fn on_download_success(
    manager: &Arc<Mutex<ModelManager>>,
    app: &AppHandle,
    asset_name: &str,
    outcome: &DownloadOutcome,
) {
    let (snapshot, manager_result) = {
        let mut guard = match manager.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut snapshot = None;

        if let Some(asset) = guard.asset_by_name_mut(asset_name) {
            let extracted_size = total_size(&outcome.final_path);
            let mut install_ok = true;

            match asset.kind {
                ModelKind::WhisperCt2 => {
                    if let Err(error) = crate::models::prepare_ct2_model_dir(&outcome.final_path) {
                        asset.status =
                            ModelStatus::Error(format!("CT2 model install incomplete: {error}"));
                        snapshot = Some(asset.clone());
                        install_ok = false;
                    }

                    // Track checksum/size against the primary model bin.
                    if let Some(model) = find_first_with_name(&outcome.final_path, "model.bin") {
                        let _ = asset.update_from_file(model);
                    }
                }
                ModelKind::WhisperOnnx | ModelKind::Parakeet => {
                    if let Some(tokens) = find_tokens_file(&outcome.final_path) {
                        let _ = asset.update_from_file(tokens);
                    }
                }
                ModelKind::Vad => {
                    if let Some(model) = find_first_with_extension(&outcome.final_path, "onnx") {
                        let _ = asset.update_from_file(model);
                    }
                }
                _ => {}
            }

            if install_ok {
                let recorded_size = if extracted_size > 0 {
                    extracted_size
                } else {
                    outcome.total_size_bytes
                };
                asset.set_size_bytes(recorded_size);
                if asset.checksum.is_none() {
                    if let Some(checksum) = &outcome.checksum {
                        asset.set_checksum(Some(checksum.clone()));
                    }
                }
                asset.status = ModelStatus::Installed;
                snapshot = Some(asset.clone());
            }
        }

        let save_result = guard.save();
        let sync_result = sync_runtime_environment(&*guard);

        (snapshot, save_result.and(sync_result))
    };

    if let Err(error) = manager_result {
        tracing::warn!("Failed to persist model updates: {error:?}");
    }

    if let Some(snapshot) = snapshot {
        emit_status(app, snapshot);
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Err(error) = state.reload_pipeline(app) {
            tracing::warn!("Failed to rebuild speech pipeline after model install: {error:?}");
        }
    }
}

fn on_download_failure(
    manager: &Arc<Mutex<ModelManager>>,
    app: &AppHandle,
    asset_name: &str,
    error: anyhow::Error,
) {
    let snapshot = {
        let mut guard = match manager.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut snapshot = None;
        if let Some(asset) = guard.asset_by_name_mut(asset_name) {
            asset.status = ModelStatus::Error(error.to_string());
            snapshot = Some(asset.clone());
        }
        if let Err(save_error) = guard.save() {
            tracing::warn!("Failed to persist model manifest after error: {save_error:?}");
        }
        snapshot
    };

    if let Some(snapshot) = snapshot {
        emit_status(app, snapshot);
    }
}

fn emit_status(app: &AppHandle, asset: ModelAsset) {
    events::emit_model_status(app, asset);
}

fn on_progress(
    manager: &Arc<Mutex<ModelManager>>,
    app: &AppHandle,
    asset_name: &str,
    downloaded: u64,
    expected: Option<u64>,
) {
    let snapshot = if let Ok(mut guard) = manager.lock() {
        if let Some(asset) = guard.asset_by_name_mut(asset_name) {
            let progress = progress_fraction(downloaded, expected);

            // Update size_bytes if we learned the total from Content-Length
            if asset.size_bytes == 0 {
                if let Some(total) = expected {
                    asset.set_size_bytes(total);
                }
            }

            asset.status = ModelStatus::Downloading {
                progress,
                downloaded_bytes: downloaded,
                total_bytes: expected,
            };
            Some(asset.clone())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(asset) = snapshot {
        emit_status(app, asset);
    }
}

fn progress_fraction(downloaded: u64, expected: Option<u64>) -> f32 {
    if let Some(total) = expected {
        if total > 0 {
            return ((downloaded as f64 / total as f64).clamp(0.0, 1.0)) as f32;
        }
    }
    0.0
}

pub fn sync_runtime_environment(manager: &ModelManager) -> Result<()> {
    sync_vad_env(manager)?;
    Ok(())
}

fn sync_vad_env(manager: &ModelManager) -> Result<()> {
    if let Some(asset) = manager.primary_asset(&ModelKind::Vad) {
        if matches!(asset.status, ModelStatus::Installed) {
            let vad_dir = asset.path(manager.root());
            if let Some(model) = find_first_with_extension(&vad_dir, "onnx") {
                std::env::set_var("SILERO_VAD_MODEL", model);
                return Ok(());
            }
        }
    }
    std::env::remove_var("SILERO_VAD_MODEL");
    Ok(())
}

fn find_tokens_file(dir: &Path) -> Option<PathBuf> {
    let default = dir.join("tokens.txt");
    if default.exists() {
        return Some(default);
    }
    let predicate = |entry: &fs::DirEntry| {
        entry
            .file_name()
            .to_str()
            .map(|name| name.contains("token"))
            .unwrap_or(false)
    };
    find_first_matching(dir, &predicate)
}

fn find_first_with_extension(dir: &Path, extension: &str) -> Option<PathBuf> {
    let predicate = |entry: &fs::DirEntry| {
        entry
            .file_name()
            .to_str()
            .map(|name| name.ends_with(extension))
            .unwrap_or(false)
    };
    find_first_matching(dir, &predicate)
}

fn find_first_with_name(dir: &Path, filename: &str) -> Option<PathBuf> {
    let direct = dir.join(filename);
    if direct.exists() {
        return Some(direct);
    }
    let predicate = |entry: &fs::DirEntry| entry.file_name().to_str() == Some(filename);
    find_first_matching(dir, &predicate)
}

// CT2 models are prepared/validated via crate::models::prepare_ct2_model_dir.

fn find_first_matching<F>(dir: &Path, predicate: &F) -> Option<PathBuf>
where
    F: Fn(&fs::DirEntry) -> bool,
{
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_first_matching(&path, predicate) {
                return Some(found);
            }
        } else if predicate(&entry) {
            return Some(path);
        }
    }
    None
}
