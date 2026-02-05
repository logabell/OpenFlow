use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ModelKind {
    ZipformerAsr,
    Whisper,
    Parakeet,
    PolishLlm,
    Vad,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ModelStatus {
    NotInstalled,
    Downloading {
        progress: f32,
        #[serde(default)]
        downloaded_bytes: u64,
        #[serde(default)]
        total_bytes: Option<u64>,
    },
    Installed,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAsset {
    pub kind: ModelKind,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    pub status: ModelStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModelSource>,
}

impl ModelAsset {
    #[must_use]
    pub fn path(&self, base_dir: &Path) -> PathBuf {
        base_dir
            .join(&self.kind_path())
            .join(format!("{}-{}", self.name, self.version))
    }

    #[must_use]
    fn kind_path(&self) -> String {
        match self.kind {
            ModelKind::ZipformerAsr => "asr/zipformer".into(),
            ModelKind::Whisper => "asr/whisper".into(),
            ModelKind::Parakeet => "asr/parakeet".into(),
            ModelKind::PolishLlm => "polish".into(),
            ModelKind::Vad => "vad".into(),
        }
    }

    pub fn set_checksum(&mut self, checksum: Option<String>) {
        self.checksum = checksum;
    }

    pub fn set_size_bytes(&mut self, size_bytes: u64) {
        self.size_bytes = size_bytes;
    }

    pub fn update_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        if path.exists() {
            let metadata = std::fs::metadata(path).context("stat asset file")?;
            self.size_bytes = metadata.len();
            let checksum = crate::models::compute_sha256(path)?;
            self.checksum = Some(checksum);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSource {
    pub uri: String,
    pub archive_format: ArchiveFormat,
    #[serde(default)]
    pub strip_prefix_components: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveFormat {
    Zip,
    TarGz,
    TarBz2,
    File,
}

pub struct ModelManager {
    root: PathBuf,
    manifest: PathBuf,
    assets: Vec<ModelAsset>,
}

impl ModelManager {
    pub fn new() -> Result<Self> {
        let root = resolve_model_dir()?;
        let manifest = root.join("manifest.json");
        let mut manager = Self {
            root,
            manifest,
            assets: vec![],
        };
        manager.load_manifest()?;
        manager.register_defaults();
        manager.save()?;
        Ok(manager)
    }

    pub fn assets(&self) -> Vec<&ModelAsset> {
        self.assets.iter().collect()
    }

    pub fn assets_mut(&mut self) -> Vec<&mut ModelAsset> {
        self.assets.iter_mut().collect()
    }

    pub fn assets_by_kind(&self, kind: &ModelKind) -> Vec<&ModelAsset> {
        self.assets
            .iter()
            .filter(|asset| &asset.kind == kind)
            .collect()
    }

    pub fn primary_asset(&self, kind: &ModelKind) -> Option<&ModelAsset> {
        self.assets_by_kind(kind).into_iter().max_by_key(|asset| {
            (
                matches!(asset.status, ModelStatus::Installed),
                asset.size_bytes,
            )
        })
    }

    pub fn asset_by_name_mut(&mut self, name: &str) -> Option<&mut ModelAsset> {
        self.assets.iter_mut().find(|asset| asset.name == name)
    }

    pub fn save(&self) -> Result<()> {
        let manifest = File::create(&self.manifest).context("create model manifest")?;
        serde_json::to_writer_pretty(manifest, &self.assets).context("write model manifest")?;
        Ok(())
    }

    pub fn uninstall(&mut self, kind: &ModelKind) -> Result<Option<ModelAsset>> {
        if let Some(asset) = self.assets.iter_mut().find(|asset| &asset.kind == kind) {
            let path = asset.path(&self.root);
            if path.exists() {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("remove model directory {}", path.display()))?;
            }
            asset.checksum = None;
            asset.size_bytes = 0;
            asset.status = ModelStatus::NotInstalled;
            let snapshot = asset.clone();
            self.save()?;
            return Ok(Some(snapshot));
        }
        Ok(None)
    }

    fn load_manifest(&mut self) -> Result<()> {
        if self.manifest.exists() {
            let manifest = File::open(&self.manifest).context("open model manifest")?;
            let assets: Vec<ModelAsset> =
                serde_json::from_reader(manifest).context("parse model manifest")?;
            self.assets = assets;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    fn register_defaults(&mut self) {
        for asset in default_assets() {
            if let Some(existing) = self
                .assets
                .iter_mut()
                .find(|current| current.name == asset.name)
            {
                // Always update source from defaults to repair stale URIs
                if existing.source.is_none() || existing.source != asset.source {
                    existing.source = asset.source.clone();
                }

                // For non-installed or error states, also update other metadata
                if matches!(
                    existing.status,
                    ModelStatus::NotInstalled | ModelStatus::Error(_)
                ) {
                    existing.kind = asset.kind.clone();
                    existing.version = asset.version.clone();
                    // Reset error status to allow fresh retry
                    if matches!(existing.status, ModelStatus::Error(_)) {
                        existing.status = ModelStatus::NotInstalled;
                    }
                }
            } else {
                self.assets.push(asset);
            }
        }
    }
}

fn resolve_model_dir() -> Result<PathBuf> {
    let project_dirs = ProjectDirs::from("com", "PushToTalk", "PushToTalk")
        .context("missing project directories")?;
    let dir = project_dirs.data_dir().join("models");
    std::fs::create_dir_all(&dir).context("create models dir")?;
    Ok(dir)
}

fn default_assets() -> Vec<ModelAsset> {
    vec![
        ModelAsset {
            kind: ModelKind::ZipformerAsr,
            name: "sherpa-onnx-zipformer-small-en".into(),
            version: "2023-06-26".into(),
            checksum: None,
            size_bytes: 0,
            status: ModelStatus::NotInstalled,
            source: Some(ModelSource {
                uri: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-zipformer-small-en-2023-06-26.tar.bz2"
                    .into(),
                archive_format: ArchiveFormat::TarBz2,
                strip_prefix_components: 0,
            }),
        },
        ModelAsset {
            kind: ModelKind::Whisper,
            name: "sherpa-onnx-whisper-tiny".into(),
            version: "tiny".into(),
            checksum: None,
            size_bytes: 0,
            status: ModelStatus::NotInstalled,
            source: Some(ModelSource {
                uri: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-tiny.tar.bz2"
                    .into(),
                archive_format: ArchiveFormat::TarBz2,
                strip_prefix_components: 0,
            }),
        },
        ModelAsset {
            kind: ModelKind::Parakeet,
            name: "sherpa-onnx-parakeet-tdt-0.6b-v2-int8".into(),
            version: "0.6b-v2-int8".into(),
            checksum: None,
            size_bytes: 0,
            status: ModelStatus::NotInstalled,
            source: Some(ModelSource {
                uri: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2"
                    .into(),
                archive_format: ArchiveFormat::TarBz2,
                strip_prefix_components: 0,
            }),
        },
        ModelAsset {
            kind: ModelKind::Vad,
            name: "silero-vad-onnx".into(),
            version: "v6".into(),
            checksum: None,
            size_bytes: 0,
            status: ModelStatus::NotInstalled,
            source: Some(ModelSource {
                uri: "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx".into(),
                archive_format: ArchiveFormat::File,
                strip_prefix_components: 0,
            }),
        },
        ModelAsset {
            kind: ModelKind::PolishLlm,
            name: "tiny-llama-1.1b-chat-q4_k_m".into(),
            version: "2024-01-01".into(),
            checksum: None,
            size_bytes: 0,
            status: ModelStatus::NotInstalled,
            source: Some(ModelSource {
                uri: "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf?download=1".into(),
                archive_format: ArchiveFormat::File,
                strip_prefix_components: 0,
            }),
        },
    ]
}
