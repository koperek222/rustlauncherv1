use crate::{fabric, java, mojang};
use anyhow::{bail, Context, Result};
use std::{collections::{HashMap, HashSet}, path::Path};

pub struct PlayOpts {
    pub version: String,
    pub fabric: bool,
    pub username: String,
    pub ram: u32,
    pub instance: String,
}

/// UUID offline identyczny jak w Javie: UUID.nameUUIDFromBytes("OfflinePlayer:<nick>")
pub fn offline_uuid(name: &str) -> String {
    let mut b = md5::compute(format!("OfflinePlayer:{name}")).0;
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub async fn install_only(c: &reqwest::Client, root: &Path, mc: &str, fabric_on: bool) -> Result<()> {
    let id = if fabric_on { fabric::install(c, root, mc).await? } else { mc.to_string() };
    let v = mojang::resolve_version(c, root, &id).await?;
    mojang::install_game(c, root, &v, mc).await
}

pub async fn play(c: &reqwest::Client, root: &Path, o: &PlayOpts) -> Result<()> {
    let id = if o.fabric {
        fabric::install(c, root, &o.version).await?
    } else {
        o.version.clone()
    };
    let v = mojang::resolve_version(c, root, &id).await?;
    let major = v.java_version.as_ref().map(|j| j.major_version).unwrap_or(21);
    let java = java::ensure_java(c, root, major).await?;
    mojang::install_game(c, root, &v, &o.version).await?;

    let game_dir = root.join("instances").join(&o.instance);
    tokio::fs::create_dir_all(game_dir.join("mods")).await?;
    let natives = root.join("natives").join(&id);
    tokio::fs::create_dir_all(&natives).await?;

    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut cp: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for lib in &v.libraries {
        if !mojang::rules_allow(&lib.rules) {
            continue;
        }
        if let Some((_, path, _)) = lib.location() {
            if seen.insert(path.clone()) {
                cp.push(root.join("libraries").join(&path).to_string_lossy().into_owned());
            }
        }
    }
    let jar = root
        .join("versions")
        .join(&o.version)
        .join(format!("{}.jar", o.version));
    cp.push(jar.to_string_lossy().into_owned());
    let classpath = cp.join(sep);

    let ai = v.asset_index.as_ref().context("Brak assetIndex")?;
    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", o.username.clone());
    vars.insert("version_name", id.clone());
    vars.insert("game_directory", game_dir.to_string_lossy().into_owned());
    vars.insert("assets_root", root.join("assets").to_string_lossy().into_owned());
    vars.insert("assets_index_name", ai.id.clone());
    vars.insert("auth_uuid", offline_uuid(&o.username));
    vars.insert("auth_access_token", "0".to_string());
    vars.insert("clientid", "0".to_string());
    vars.insert("auth_xuid", "0".to_string());
    vars.insert("user_type", "legacy".to_string());
    vars.insert("version_type", "RustLauncher".to_string());
    vars.insert("natives_directory", natives.to_string_lossy().into_owned());
    vars.insert("launcher_name", "RustLauncher".to_string());
    vars.insert("launcher_version", "0.1".to_string());
    vars.insert("classpath", classpath);

    let sub = |s: &str| -> String {
        let mut out = s.to_string();
        for (k, val) in &vars {
            out = out.replace(&format!("${{{k}}}"), val);
        }
        out
    };

    let args = v.arguments.as_ref().context("Ta wersja nie ma sekcji 'arguments'")?;
    let jvm = mojang::resolve_args(&args.jvm);
    let game = mojang::resolve_args(&args.game);
    let main_class = v.main_class.clone().context("Brak mainClass")?;
    if jvm.is_empty() {
        bail!("Pusta lista argumentów JVM - nieobsługiwany format wersji");
    }

    println!("Uruchamianie {id} jako {} ...", o.username);
    let mut cmd = tokio::process::Command::new(&java);
    cmd.current_dir(&game_dir)
        .arg(format!("-Xmx{}M", o.ram))
        .arg("-Xms512M");
    for a in &jvm {
        cmd.arg(sub(a));
    }
    cmd.arg(main_class);
    for a in &game {
        cmd.arg(sub(a));
    }
    let status = cmd.status().await?;
    println!("Gra zakończona ({status}).");
    Ok(())
}
