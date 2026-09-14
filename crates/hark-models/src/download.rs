//! Resumable, verified model downloads.

use crate::catalog::{model_path, ModelSpec};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    Checksum { file: String, expected: String, actual: String },
    #[error("cancelled")]
    Cancelled,
}

/// SHA-256 of a file, hex-encoded. Streams in 1 MiB chunks.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn verify_sha256(path: &Path, expected: &str) -> std::io::Result<bool> {
    Ok(sha256_file(path)?.eq_ignore_ascii_case(expected))
}

/// Download `spec` into `models_dir`, resuming a `.part` file if present.
/// `progress(done, total)` is called as bytes arrive; `cancel()` is polled
/// between chunks. The finished file is renamed into place only after the
/// SHA-256 matches.
pub async fn download(
    spec: &ModelSpec,
    models_dir: &Path,
    progress: impl Fn(u64, u64) + Send,
    cancel: impl Fn() -> bool + Send,
) -> Result<PathBuf, DownloadError> {
    tokio::fs::create_dir_all(models_dir).await?;
    let final_path = model_path(models_dir, spec);
    if final_path.exists() && verify_sha256(&final_path, &spec.sha256)? {
        progress(spec.size_bytes, spec.size_bytes);
        return Ok(final_path);
    }
    let part = final_path.with_extension(format!(
        "{}.part",
        final_path.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    let mut done = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if done > spec.size_bytes {
        tokio::fs::remove_file(&part).await?;
        done = 0;
    }

    let client = reqwest::Client::builder().user_agent("hark/0.1 (+https://github.com/hark-app/hark)").build()?;
    let mut req = client.get(&spec.url);
    if done > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={done}-"));
    }
    let resp = req.send().await?.error_for_status()?;
    let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed {
        done = 0;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(resumed)
        .write(true)
        .truncate(!resumed)
        .open(&part)
        .await?;

    let total = spec.size_bytes;
    let mut stream = resp.bytes_stream();
    progress(done, total);
    while let Some(chunk) = stream.next().await {
        if cancel() {
            file.flush().await?;
            return Err(DownloadError::Cancelled);
        }
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await?;
    drop(file);

    let part_for_hash = part.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_file(&part_for_hash))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    if !actual.eq_ignore_ascii_case(&spec.sha256) {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(DownloadError::Checksum { file: spec.file.clone(), expected: spec.sha256.clone(), actual });
    }
    tokio::fs::rename(&part, &final_path).await?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_content() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("hello.txt");
        std::fs::write(&p, b"hello").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert!(verify_sha256(&p, "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824").unwrap());
        assert!(!verify_sha256(&p, "00").unwrap());
    }
}
