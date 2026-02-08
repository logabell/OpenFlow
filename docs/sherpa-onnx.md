# Sherpa-ONNX Model Notes

## Recommended Streaming Packages

- Visit [`k2-fsa.github.io/sherpa/onnx`](https://k2-fsa.github.io/sherpa/onnx/index.html) and choose a **Streaming Zipformer (Transducer)** build for your language.  
- For English, we seed two catalog entries that map to the official GitHub release assets:
  - `sherpa-onnx-streaming-zipformer-en-20M-2023-02-17.tar.bz2` (~122 MB download, 128 MB on disk).
  - `sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2` (~296 MB download, 310 MB on disk).
- Each archive extracts into a folder with four assets:
  - `encoder-*.onnx`
  - `decoder-*.onnx`
  - `joiner-*.onnx`
  - `tokens.txt`

The names carry epoch metadata (`encoder-epoch-99-avg-1.onnx`, etc.). The backend auto-discovers those files, so renaming is no longer required.

## Environment Configuration

Set the following when launching the Tauri backend (shell examples assume bash on Linux). Remember to enable the `asr-sherpa` feature when building (`cargo tauri dev --features asr-sherpa` or set it in the default feature list):

```bash
export SHERPA_ONLINE_MODEL="$HOME/Models/sherpa-onnx-streaming-zipformer-en-2023-06-26"
# Optional: override if tokens live outside the model dir
# export SHERPA_ONLINE_TOKENS="$HOME/Models/sherpa-onnx-streaming-zipformer-en-2023-06-26/tokens.txt"
```

`SHERPA_ONLINE_MODEL` must point to the extracted directory that contains the ONNX triplet and `tokens.txt`. If `SHERPA_ONLINE_TOKENS` is unset, the backend falls back to scanning the model directory for `tokens.txt` (or any `*token*.txt` file).

## Future Automation Ideas

- Extend `ModelManager` with concrete manifests (URL, checksum, size) for the preferred Zipformer builds and queue them for download via the Settings UI.
- Bundle a small helper script/CI job that mirrors the “streaming Zipformer (en)” artifacts into internal storage to speed up installs.
- Add validation to surface a clear HUD warning when the ONNX files are missing or damaged (after checksum verification).

## Catalog Integration

- `ModelManager` seeds two default streaming entries, tracking the `20M` footprint and the full `2023-06-26` English Zipformer release. Each asset stores the GitHub release URL plus archive format (`tar.bz2`) so a future downloader can unpack straight into the managed model cache.
- `checksum` is optional; compute and persist the SHA-256 in the manifest once we have a trusted hash (GitHub releases don't publish one today).
- Primary selection favors installed assets first; otherwise the larger (full) download becomes the default streaming engine exposed to callers.
- When the downloader lands, point it at `ModelAsset::source.uri`, verify the checksum if present, expand the archive into `models/streaming/<name>-<version>`, and set `SHERPA_ONLINE_MODEL` to that directory. Leave `strip_prefix_components` at `0` because the archive already contains a single top-level directory that matches our final path.
- After a download, call `ModelAsset::update_from_file()` to persist the measured size plus the SHA-256 generated via `models::compute_sha256` so subsequent runs can verify the cache without re-downloading.

## Downloader Requirements (TBD)

1. **Acquisition**: Support HTTP range downloads with resume + progress events so the frontend can surface a deterministic percentage. `size_bytes` gives the rough target for progress bars even when the server omits `Content-Length`.
2. **Verification**: Compute SHA-256 after download. If `checksum` is `Some`, enforce strict equality; if `None`, persist the newly computed hash back into the manifest for future runs.
3. **Extraction**: Handle `tar.bz2` and `tar.gz` via streaming to avoid double-storing archives. Respect `strip_prefix_components` before writing into `ModelAsset::path(root)`.
4. **Registration**: Update the cached `ModelAsset` status to `Installed` on success and emit a settings event so the UI can show readiness. Failed downloads should move to `ModelStatus::Error(String)`.
5. **Cleanup**: Remove temporary files and partially extracted directories on failure to keep the cache tidy.
6. **Plan Interface**: `models::build_download_plan(asset, model_dir)` returns a `DownloadPlan` (URI, archive type, destination, expected size/checksum) that the downloader executes via `models::download_and_extract`, returning a `DownloadOutcome` (archive path, checksum, bytes). Once the archive lands, call `ModelAsset::update_from_file` with any critical payloads to persist `checksum` and `size_bytes`.

## Runtime Integration

- The backend exposes `list_models`, `install_streaming_asr`, `install_vad_model`, `uninstall_streaming_asr`, and `uninstall_vad_model` commands for the React shell. Streaming installs queue `ModelKind::StreamingAsr`, while VAD pulls `silero_vad.onnx` into the catalog and uninstall commands remove on-disk assets + manifest entries.
- `model-status` events broadcast every status transition (`NotInstalled`, `Downloading`, `Installed`, `Error`) with the latest size/checksum metadata so the UI can surface progress and completion states.
- Environment variables (`SHERPA_ONLINE_MODEL`, `SHERPA_ONLINE_TOKENS`, `SILERO_VAD_MODEL`) are synced automatically whenever an install completes or on startup, enabling `asr-sherpa`/`vad-silero` feature gates without manual configuration.
- Catalog metadata now persists to `models/manifest.json`; restarts reload this manifest before the download worker spins up, preserving install state and hashes.
