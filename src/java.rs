use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

fn exe() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn java_major(p: &Path) -> Option<u32> {
    let out = std::process::Command::new(p).arg("-version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    let start = text.find('"')? + 1;
    let rest = &text[start..];
    let end = rest.find('"')?;
    let ver = &rest[..end];
    let mut parts = ver.split(['.', '_', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn system_java(major: u32) -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Ok(h) = std::env::var("JAVA_HOME") {
        cands.push(PathBuf::from(h).join("bin").join(exe()));
    }
    cands.push(PathBuf::from("java"));
    for c in cands {
        if let Some(m) = java_major(&c) {
            if m >= major {
                return Some(c);
            }
        }
    }
    None
}

fn find_java_bin(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(found) = find_java_bin(&p) {
                return Some(found);
            }
        } else if p.file_name().map(|n| n == exe()).unwrap_or(false)
            && p.parent().and_then(|x| x.file_name()).map(|n| n == "bin").unwrap_or(false)
        {
            return Some(p);
        }
    }
    None
}

async fn download_temurin(c: &reqwest::Client, dir: &Path, major: u32) -> Result<()> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" };
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{major}/ga/{os}/{arch}/jre/hotspot/normal/eclipse"
    );
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let tmp = dir.with_extension(ext);
    let bytes = c.get(&url).send().await?.error_for_status()?.bytes().await?;
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(&tmp, &bytes).await?;
    let (t, d) = (tmp.clone(), dir.to_path_buf());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let f = std::fs::File::open(&t)?;
        if cfg!(windows) {
            zip::ZipArchive::new(f)?.extract(&d)?;
        } else {
            tar::Archive::new(flate2::read::GzDecoder::new(f)).unpack(&d)?;
        }
        Ok(())
    })
    .await??;
    let _ = tokio::fs::remove_file(&tmp).await;
    Ok(())
}

pub async fn ensure_java(c: &reqwest::Client, root: &Path, major: u32) -> Result<PathBuf> {
    let dir = root.join("runtime").join(format!("java-{major}"));
    if let Some(p) = find_java_bin(&dir) {
        return Ok(p);
    }
    if let Some(p) = system_java(major) {
        println!("Używam systemowej Javy: {}", p.display());
        return Ok(p);
    }
    println!("Pobieranie Javy {major} (Eclipse Temurin)...");
    download_temurin(c, &dir, major).await?;
    find_java_bin(&dir).context("Nie udało się znaleźć pliku java po rozpakowaniu")
}
