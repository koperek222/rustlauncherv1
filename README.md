# RustLauncher

Prosty launcher Minecraft Java napisany w Rust. Domyślnie Minecraft 1.21.11 + Fabric.

## Co umie
- pobiera wersję, biblioteki i assety od Mojang (z weryfikacją SHA-1, równolegle)
- sam znajdzie Javę (systemową) albo pobierze Temurin JRE do własnego folderu
- instaluje Fabric Loader
- instaluje mody z Modrinth razem z wymaganymi zależnościami (np. Fabric API)
- tryb offline (nick dowolny); logowanie Microsoft nie jest jeszcze zaimplementowane

## Jak zdobyć program
**Bez instalowania Rusta:** wrzuć ten folder na GitHub (repo publiczne lub prywatne),
wejdź w zakładkę *Actions* → *build* → pobierz artefakt dla swojego systemu (.exe dla Windows).

**Z Rustem:** `cargo build --release` → `target/release/rustlauncher(.exe)`

## Użycie
Uruchom bez argumentów — pojawi się proste menu (Graj / Mody / Ustawienia).

CLI:
```
rustlauncher list
rustlauncher install 1.21.11 --fabric
rustlauncher mods add sodium lithium --mc 1.21.11
rustlauncher play --mc 1.21.11 --fabric -u TwojNick --ram 4096
```
Dane trafiają do `%APPDATA%\RustLauncher\data` (Windows) lub `~/.local/share/RustLauncher` (Linux); zmiana: `--dir`.
Mody ręcznie wrzucasz do `instances/default/mods`.

## Mody Java → Rust
Nie da się ich konwertować (gra działa na JVM, mody to bytecode Javy + Mixiny).
Launcher uruchamia zwykłe mody Fabric w Javie.
