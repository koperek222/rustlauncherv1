use crate::util::download;
use anyhow::{Context, Result};
use std::path::Path;

/// Instaluje profil Fabric Loader dla danej wersji MC. Zwraca id profilu.
pub async fn install(c: &reqwest::Client, root: &Path, mc: &str) -> Result<String> {
    let vdir = root.join("versions");
    let suffix = format!("-{mc}");
    if let Ok(rd) = std::fs::read_dir(&vdir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("fabric-loader-")
                && n.ends_with(&suffix)
                && e.path().join(format!("{n}.json")).exists()
            {
                return Ok(n);
            }
        }
    }
    println!("Pobieranie profilu Fabric dla {mc}...");
    let list: Vec<serde_json::Value> = c
        .get(format!("https://meta.fabricmc.net/v2/versions/loader/{mc}"))
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Fabric nie wspiera (jeszcze) wersji {mc}"))?
        .json()
        .await?;
    let entry = list
        .iter()
        .find(|e| e["loader"]["stable"].as_bool() == Some(true))
        .or(list.first())
        .with_context(|| format!("Fabric nie ma loadera dla {mc}"))?;
    let loader = entry["loader"]["version"]
        .as_str()
        .context("Brak wersji loadera w odpowiedzi Fabric")?;
    let id = format!("fabric-loader-{loader}-{mc}");
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json");
    let path = vdir.join(&id).join(format!("{id}.json"));
    download(c, &url, &path, None, None).await?;
    Ok(id)
}
