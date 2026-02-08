use std::{
    fs,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub fn compute_sha256(path: &Path) -> Result<String> {
    let file =
        File::open(path).with_context(|| format!("open file for hashing: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader.read(&mut buffer).context("hash read")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

pub fn total_size(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    }
    let mut size: u64 = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            size = size.saturating_add(total_size(&entry.path()));
        }
    }
    size
}
