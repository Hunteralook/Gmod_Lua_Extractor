//! Поиск папки `garrysmod/cache/lua` в библиотеках Steam.

use std::path::{Path, PathBuf};

/// Возможные корни установки Steam для текущей ОС.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if cfg!(target_os = "windows") {
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(dir) = std::env::var(var) {
                roots.push(Path::new(&dir).join("Steam"));
            }
        }
        if let Ok(dir) = std::env::var("SystemDrive") {
            roots.push(Path::new(&dir).join("Steam"));
        }
    } else if let Some(home) = home_dir() {
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join("Library/Application Support/Steam"));
    }

    roots
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Все папки библиотек Steam, включая перечисленные в `libraryfolders.vdf`.
fn library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![steam_root.to_path_buf()];
    let vdf = steam_root.join("steamapps/libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(&vdf) else {
        return folders;
    };

    // Простой разбор без зависимостей: ищем строки вида  "path"  "C:\\...".
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("\"path\"") {
            if let Some(path) = extract_quoted(rest) {
                folders.push(PathBuf::from(path.replace("\\\\", "\\")));
            }
        }
    }
    folders
}

/// Возвращает содержимое первой строки в кавычках из `s`.
fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let end = s[start..].find('"')? + start;
    Some(s[start..end].to_string())
}

/// Ищет все существующие папки `garrysmod/cache/lua`.
pub fn find_cache_dirs() -> Vec<PathBuf> {
    let suffix = Path::new("steamapps/common/GarrysMod/garrysmod/cache/lua");
    let mut found: Vec<PathBuf> = Vec::new();

    for root in steam_roots() {
        for library in library_folders(&root) {
            let cache = library.join(suffix);
            if cache.is_dir() {
                let canonical = cache.canonicalize().unwrap_or_else(|_| cache.clone());
                if !found
                    .iter()
                    .any(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == canonical)
                {
                    found.push(cache);
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::extract_quoted;

    #[test]
    fn parses_quoted_path() {
        assert_eq!(
            extract_quoted("   \"D:\\\\SteamLibrary\"").as_deref(),
            Some("D:\\\\SteamLibrary")
        );
        assert_eq!(extract_quoted("no quotes"), None);
    }
}
