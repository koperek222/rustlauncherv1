use crate::util::{download, download_many, DlTask};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Deserialize)]
pub struct Manifest {
    pub versions: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Ruled {
        #[serde(default)]
        rules: Vec<Rule>,
        value: ArgValue,
    },
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub url: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Downloads {
    pub client: Option<Artifact>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub major_version: u32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
}

#[derive(Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObj>,
}

#[derive(Deserialize)]
struct AssetObj {
    hash: String,
    size: u64,
}

pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub fn os_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "unknown"
    }
}

pub fn rules_allow(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in rules {
        let mut matches = true;
        if let Some(os) = &r.os {
            if let Some(n) = &os.name {
                if n != os_name() {
                    matches = false;
                }
            }
            if let Some(a) = &os.arch {
                if a != os_arch() {
                    matches = false;
                }
            }
        }
        if let Some(f) = &r.features {
            // żadne "features" (demo, własna rozdzielczość, quick play) nie są włączone
            if !f.values().all(|v| !*v) {
                matches = false;
            }
        }
        if matches {
            allowed = r.action == "allow";
        }
    }
    allowed
}

pub fn resolve_args(args: &[Arg]) -> Vec<String> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Arg::Plain(s) => out.push(s.clone()),
            Arg::Ruled { rules, value } => {
                if rules_allow(rules) {
                    match value {
                        ArgValue::One(s) => out.push(s.clone()),
                        ArgValue::Many(v) => out.extend(v.iter().cloned()),
                    }
                }
            }
        }
    }
    out
}

pub fn maven_path(name: &str) -> String {
    let (coords, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let p: Vec<&str> = coords.split(':').collect();
    let group = p[0].replace('.', "/");
    let (a, v) = (p[1], p[2]);
    let file = if p.len() > 3 {
        format!("{a}-{v}-{}.{ext}", p[3])
    } else {
        format!("{a}-{v}.{ext}")
    };
    format!("{group}/{a}/{v}/{file}")
}

impl Library {
    /// (url, ścieżka względna w libraries/, sha1)
    pub fn location(&self) -> Option<(String, String, Option<String>)> {
        if let Some(d) = &self.downloads {
            let a = d.artifact.as_ref()?;
            let path = a.path.clone().unwrap_or_else(|| maven_path(&self.name));
            return Some((a.url.clone(), path, a.sha1.clone()));
        }
        let mut base = self
            .url
            .clone()
            .unwrap_or_else(|| "https://libraries.minecraft.net/".to_string());
        if !base.ends_with('/') {
            base.push('/');
        }
        let path = maven_path(&self.name);
        Some((format!("{base}{path}"), path, None))
    }
}

pub async fn fetch_manifest(c: &reqwest::Client) -> Result<Manifest> {
    Ok(c.get(MANIFEST_URL).send().await?.error_for_status()?.json().await?)
}

async fn read_or_fetch(c: &reqwest::Client, root: &Path, id: &str) -> Result<VersionJson> {
    let path = root.join("versions").join(id).join(format!("{id}.json"));
    if !path.exists() {
        let m = fetch_manifest(c).await?;
        let e = m
            .versions
            .iter()
            .find(|v| v.id == id)
            .with_context(|| format!("Nie znaleziono wersji '{id}' w manifeście Mojang"))?;
        download(c, &e.url, &path, None, None).await?;
    }
    let data = tokio::fs::read(&path).await?;
    Ok(serde_json::from_slice(&data)?)
}

fn merge(parent: VersionJson, child: VersionJson) -> VersionJson {
    let mut libs = child.libraries;
    libs.extend(parent.libraries);
    let mut seen = HashSet::new();
    libs.retain(|l| seen.insert(l.name.clone()));
    let pa = parent.arguments.unwrap_or_default();
    let ca = child.arguments.unwrap_or_default();
    let mut game = pa.game;
    game.extend(ca.game);
    let mut jvm = pa.jvm;
    jvm.extend(ca.jvm);
    VersionJson {
        id: child.id,
        inherits_from: None,
        main_class: child.main_class.or(parent.main_class),
        asset_index: child.asset_index.or(parent.asset_index),
        downloads: child.downloads.or(parent.downloads),
        libraries: libs,
        arguments: Some(Arguments { game, jvm }),
        java_version: child.java_version.or(parent.java_version),
    }
}

/// Wczytuje wersję (pobierając JSON jeśli trzeba) i scala z rodzicem (Fabric -> vanilla).
pub async fn resolve_version(c: &reqwest::Client, root: &Path, id: &str) -> Result<VersionJson> {
    let child = read_or_fetch(c, root, id).await?;
    if let Some(p) = child.inherits_from.clone() {
        let parent = read_or_fetch(c, root, &p).await?;
        Ok(merge(parent, child))
    } else {
        Ok(child)
    }
}

pub async fn install_game(c: &reqwest::Client, root: &Path, v: &VersionJson, mc_id: &str) -> Result<()> {
    let client = v
        .downloads
        .as_ref()
        .and_then(|d| d.client.as_ref())
        .context("Brak client.jar w JSON wersji")?;
    let jar = root.join("versions").join(mc_id).join(format!("{mc_id}.jar"));
    println!("Sprawdzanie client.jar...");
    download(c, &client.url, &jar, client.sha1.as_deref(), None).await?;

    let mut tasks = Vec::new();
    for lib in &v.libraries {
        if !rules_allow(&lib.rules) {
            continue;
        }
        if let Some((url, path, sha1)) = lib.location() {
            tasks.push(DlTask {
                url,
                path: root.join("libraries").join(path),
                sha1,
                size: None,
            });
        }
    }
    download_many(c, tasks, "Biblioteki").await?;

    let ai = v.asset_index.as_ref().context("Brak assetIndex w JSON wersji")?;
    let idx_path = root.join("assets").join("indexes").join(format!("{}.json", ai.id));
    download(c, &ai.url, &idx_path, Some(&ai.sha1), None).await?;
    let idx: AssetIndex = serde_json::from_slice(&tokio::fs::read(&idx_path).await?)?;
    let mut seen = HashSet::new();
    let mut tasks = Vec::new();
    for (_name, obj) in idx.objects {
        if !seen.insert(obj.hash.clone()) {
            continue;
        }
        let prefix = obj.hash[..2].to_string();
        tasks.push(DlTask {
            url: format!("https://resources.download.minecraft.net/{prefix}/{}", obj.hash),
            path: root.join("assets").join("objects").join(&prefix).join(&obj.hash),
            sha1: Some(obj.hash.clone()),
            size: Some(obj.size),
        });
    }
    download_many(c, tasks, "Assety").await?;
    Ok(())
}
