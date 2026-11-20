use crate::util::download;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{collections::HashSet, path::Path};

#[derive(Deserialize, Default)]
struct MrHashes {
    sha1: Option<String>,
}

#[derive(Deserialize)]
struct MrFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    hashes: MrHashes,
}

#[derive(Deserialize)]
struct MrDep {
    project_id: Option<String>,
    dependency_type: String,
}

#[derive(Deserialize)]
struct MrVersion {
    version_type: String,
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: Vec<MrDep>,
}

/// Instaluje mody z Modrinth wraz z wymaganymi zależnościami (np. Fabric API).
pub async fn install(
    c: &reqwest::Client,
    mods_dir: &Path,
    slugs: &[String],
    mc: &str,
    loader: &str,
) -> Result<()> {
    tokio::fs::create_dir_all(mods_dir).await?;
    let mut queue: Vec<(String, bool)> = slugs.iter().map(|s| (s.clone(), true)).collect();
    queue.reverse();
    let mut seen = HashSet::new();
    while let Some((proj, is_root)) = queue.pop() {
        if !seen.insert(proj.clone()) {
            continue;
        }
        let url = format!("https://api.modrinth.com/v2/project/{proj}/version");
        let resp = c
            .get(&url)
            .query(&[
                ("game_versions", format!("[\"{mc}\"]")),
                ("loaders", format!("[\"{loader}\"]")),
            ])
            .send()
            .await?;
        if !resp.status().is_success() {
            if is_root {
                bail!("Nie znaleziono projektu '{proj}' na Modrinth ({})", resp.status());
            }
            println!("  ! pominięto zależność {proj} ({})", resp.status());
            continue;
        }
        let versions: Vec<MrVersion> = resp.json().await?;
        let pick = versions
            .iter()
            .find(|v| v.version_type == "release")
            .or(versions.first());
        let v = match pick {
            Some(v) => v,
            None => {
                if is_root {
                    bail!("'{proj}' nie ma wersji dla Minecraft {mc} ({loader})");
                }
                println!("  ! zależność {proj} nie ma wersji dla {mc}");
                continue;
            }
        };
        let f = v
            .files
            .iter()
            .find(|f| f.primary)
            .or(v.files.first())
            .context("Wersja moda nie ma plików")?;
        download(c, &f.url, &mods_dir.join(&f.filename), f.hashes.sha1.as_deref(), None).await?;
        println!("  ✓ {}", f.filename);
        for d in &v.dependencies {
            if d.dependency_type == "required" {
                if let Some(p) = &d.project_id {
                    queue.push((p.clone(), false));
                }
            }
        }
    }
    Ok(())
}
