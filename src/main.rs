mod fabric;
mod java;
mod launch;
mod modrinth;
mod mojang;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use launch::PlayOpts;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "rustlauncher", about = "Launcher Minecraft Java napisany w Rust (Fabric + Modrinth)")]
struct Cli {
    /// Katalog danych launchera (domyślnie katalog użytkownika)
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lista wersji Minecraft
    List {
        #[arg(long)]
        snapshots: bool,
    },
    /// Pobierz wersję (bez uruchamiania)
    Install {
        #[arg(default_value = "1.21.11")]
        mc: String,
        #[arg(long)]
        fabric: bool,
    },
    /// Pobierz (jeśli trzeba) i uruchom grę (tryb offline)
    Play {
        #[arg(long, default_value = "1.21.11")]
        mc: String,
        #[arg(long)]
        fabric: bool,
        #[arg(short, long, default_value = "Player")]
        username: String,
        #[arg(long, default_value_t = 4096)]
        ram: u32,
        #[arg(long, default_value = "default")]
        instance: String,
    },
    /// Zarządzanie modami (Modrinth, Fabric)
    Mods {
        #[command(subcommand)]
        action: ModsCmd,
    },
}

#[derive(Subcommand)]
enum ModsCmd {
    /// Zainstaluj mody po slugach z Modrinth (np. sodium lithium)
    Add {
        slugs: Vec<String>,
        #[arg(long, default_value = "1.21.11")]
        mc: String,
        #[arg(long, default_value = "default")]
        instance: String,
    },
    /// Pokaż zainstalowane mody
    List {
        #[arg(long, default_value = "default")]
        instance: String,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    username: String,
    ram: u32,
    version: String,
    fabric: bool,
    instance: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            username: "Player".into(),
            ram: 4096,
            version: "1.21.11".into(),
            fabric: true,
            instance: "default".into(),
        }
    }
}

impl Config {
    fn load(root: &Path) -> Config {
        std::fs::read(root.join("config.json"))
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }
    fn save(&self, root: &Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(root.join("config.json"), s);
        }
    }
}

fn prompt(msg: &str) -> String {
    use std::io::Write;
    print!("{msg}");
    std::io::stdout().flush().ok();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).ok();
    s.trim().to_string()
}

fn mods_dir(root: &Path, instance: &str) -> PathBuf {
    root.join("instances").join(instance).join("mods")
}

fn list_mods(root: &Path, instance: &str) {
    let dir = mods_dir(root, instance);
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".jar") {
                println!("  - {name}");
                n += 1;
            }
        }
    }
    println!("Razem: {n} (folder: {})", dir.display());
}

async fn interactive(c: &reqwest::Client, root: PathBuf) -> Result<()> {
    let mut cfg = Config::load(&root);
    loop {
        println!(
            "\n=== RustLauncher ===\nNick: {} | Wersja: {}{} | RAM: {} MB",
            cfg.username,
            cfg.version,
            if cfg.fabric { " + Fabric" } else { "" },
            cfg.ram
        );
        println!("1) Graj\n2) Zainstaluj mody z Modrinth\n3) Lista modów\n4) Ustawienia\n0) Wyjście");
        let choice = prompt("> ");
        let res: Result<()> = async {
            match choice.as_str() {
                "1" => {
                    launch::play(
                        c,
                        &root,
                        &PlayOpts {
                            version: cfg.version.clone(),
                            fabric: cfg.fabric,
                            username: cfg.username.clone(),
                            ram: cfg.ram,
                            instance: cfg.instance.clone(),
                        },
                    )
                    .await?;
                }
                "2" => {
                    let s = prompt("Slugi modów oddzielone spacją (np. sodium lithium iris): ");
                    let slugs: Vec<String> = s.split_whitespace().map(|x| x.to_string()).collect();
                    if !slugs.is_empty() {
                        if !cfg.fabric {
                            println!("Uwaga: mody wymagają Fabric - włącz go w ustawieniach.");
                        }
                        modrinth::install(c, &mods_dir(&root, &cfg.instance), &slugs, &cfg.version, "fabric").await?;
                    }
                }
                "3" => list_mods(&root, &cfg.instance),
                "4" => {
                    let n = prompt(&format!("Nick [{}]: ", cfg.username));
                    if !n.is_empty() {
                        cfg.username = n;
                    }
                    let v = prompt(&format!("Wersja MC [{}]: ", cfg.version));
                    if !v.is_empty() {
                        cfg.version = v;
                    }
                    let r = prompt(&format!("RAM w MB [{}]: ", cfg.ram));
                    if let Ok(r) = r.parse::<u32>() {
                        cfg.ram = r;
                    }
                    let f = prompt(&format!("Fabric (t/n) [{}]: ", if cfg.fabric { "t" } else { "n" }));
                    if f.eq_ignore_ascii_case("t") {
                        cfg.fabric = true;
                    } else if f.eq_ignore_ascii_case("n") {
                        cfg.fabric = false;
                    }
                    cfg.save(&root);
                }
                _ => {}
            }
            Ok(())
        }
        .await;
        if choice == "0" {
            break;
        }
        if let Err(e) = res {
            println!("Błąd: {e:#}");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.dir.unwrap_or_else(|| {
        directories::ProjectDirs::from("", "", "RustLauncher")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("./minecraft"))
    });
    std::fs::create_dir_all(&root)?;
    let c = util::client()?;

    match cli.cmd {
        None => interactive(&c, root).await?,
        Some(Cmd::List { snapshots }) => {
            let m = mojang::fetch_manifest(&c).await?;
            for v in m.versions.iter().filter(|v| snapshots || v.kind == "release") {
                println!("{} ({})", v.id, v.kind);
            }
        }
        Some(Cmd::Install { mc, fabric }) => launch::install_only(&c, &root, &mc, fabric).await?,
        Some(Cmd::Play { mc, fabric, username, ram, instance }) => {
            launch::play(&c, &root, &PlayOpts { version: mc, fabric, username, ram, instance }).await?
        }
        Some(Cmd::Mods { action }) => match action {
            ModsCmd::Add { slugs, mc, instance } => {
                modrinth::install(&c, &mods_dir(&root, &instance), &slugs, &mc, "fabric").await?
            }
            ModsCmd::List { instance } => list_mods(&root, &instance),
        },
    }
    Ok(())
}
