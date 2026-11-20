use anyhow::{bail, Result};
use futures::{stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("rustlauncher/0.1 (hobby launcher)")
        .build()?)
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

async fn file_ok(path: &Path, sha1: Option<&str>, size: Option<u64>) -> bool {
    let Ok(meta) = tokio::fs::metadata(path).await else {
        return false;
    };
    if let Some(sz) = size {
        return meta.len() == sz;
    }
    match sha1 {
        Some(s) => tokio::fs::read(path)
            .await
            .map(|d| sha1_hex(&d).eq_ignore_ascii_case(s))
            .unwrap_or(false),
        None => true,
    }
}

async fn try_download(c: &reqwest::Client, url: &str, path: &Path, sha1: Option<&str>) -> Result<()> {
    let resp = c.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    if let Some(s) = sha1 {
        let got = sha1_hex(&bytes);
        if !got.eq_ignore_ascii_case(s) {
            bail!("Zła suma SHA-1 dla {url}");
        }
    }
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, &bytes).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

/// Pobiera plik (pomija, jeśli już jest i się zgadza). 3 próby.
pub async fn download(
    c: &reqwest::Client,
    url: &str,
    path: &Path,
    sha1: Option<&str>,
    size: Option<u64>,
) -> Result<()> {
    if file_ok(path, sha1, size).await {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let mut last = None;
    for _ in 0..3 {
        match try_download(c, url, path, sha1).await {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

pub struct DlTask {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

pub async fn download_many(c: &reqwest::Client, tasks: Vec<DlTask>, label: &str) -> Result<()> {
    let pb = ProgressBar::new(tasks.len() as u64);
    pb.set_style(ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")?.progress_chars("=> "));
    pb.set_message(label.to_string());
    let results: Vec<Result<()>> = stream::iter(tasks)
        .map(|t| {
            let c = c.clone();
            let pb = pb.clone();
            async move {
                let r = download(&c, &t.url, &t.path, t.sha1.as_deref(), t.size).await;
                pb.inc(1);
                r
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    pb.finish();
    for r in results {
        r?;
    }
    Ok(())
}
