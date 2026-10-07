//! Gmod Lua Extractor — распаковщик клиентских Lua-скриптов из кэша Garry's Mod.
//!
//! Утилита не внедряется в игру. Она читает файлы, которые игра сама сохранила
//! в `garrysmod/cache/lua` при подключении к серверу, распаковывает их и
//! складывает в указанную папку.

mod cache;
mod steam;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Отчёт о распаковке одной папки кэша.
#[derive(Default)]
struct Report {
    extracted: usize,
    duplicates: usize,
    failed: Vec<(PathBuf, String)>,
}

struct Options {
    cache: Option<PathBuf>,
    output: PathBuf,
    dedupe: bool,
    verbose: bool,
}

fn print_help() {
    println!(
        "Gmod Lua Extractor — распаковщик кэша клиентских Lua-скриптов Garry's Mod\n\n\
         Использование:\n  \
         gmod_lua_extractor [ПАПКА_КЭША] [параметры]\n\n\
         Аргументы:\n  \
         ПАПКА_КЭША            путь к cache/lua; если не указан — ищется автоматически\n\n\
         Параметры:\n  \
         -o, --output ПАПКА    куда сохранить скрипты (по умолчанию ./extracted_lua)\n  \
         --keep-duplicates     не пропускать файлы с одинаковым содержимым\n  \
         -v, --verbose         подробный вывод\n  \
         -h, --help            показать эту справку"
    );
}

/// Разбирает аргументы командной строки.
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        cache: None,
        output: PathBuf::from("extracted_lua"),
        dedupe: true,
        verbose: false,
    };

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("{arg}: ожидается путь"))?;
                opts.output = PathBuf::from(value);
            }
            "--keep-duplicates" => opts.dedupe = false,
            "-v" | "--verbose" => opts.verbose = true,
            other if other.starts_with('-') => {
                return Err(format!("неизвестный параметр: {other}"));
            }
            other => {
                if opts.cache.is_some() {
                    return Err(format!("лишний аргумент: {other}"));
                }
                opts.cache = Some(PathBuf::from(other));
            }
        }
    }
    Ok(opts)
}

/// 40-символьный hex SHA-1 от содержимого — для отсева дубликатов.
fn content_digest(data: &[u8]) -> String {
    let hash = Sha1::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn extract_cache(cache_dir: &Path, opts: &Options) -> std::io::Result<Report> {
    std::fs::create_dir_all(&opts.output)?;
    let mut report = Report::default();
    let mut seen: HashSet<String> = HashSet::new();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(cache_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    for path in entries {
        let raw = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.failed.push((path.clone(), error.to_string()));
                continue;
            }
        };

        let source = match cache::decode_cache_file(&raw) {
            Ok(data) => data,
            Err(error) => {
                if opts.verbose {
                    eprintln!("[!] {}: {error}", path.display());
                }
                report.failed.push((path.clone(), error.to_string()));
                continue;
            }
        };

        if opts.dedupe {
            let digest = content_digest(&source);
            if !seen.insert(digest) {
                report.duplicates += 1;
                continue;
            }
        }

        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let target = opts.output.join(format!("{stem}.lua"));
        if let Err(error) = std::fs::write(&target, &source) {
            report.failed.push((path.clone(), error.to_string()));
            continue;
        }
        report.extracted += 1;
        if opts.verbose {
            println!(
                "[+] {} -> {} ({} байт)",
                path.file_name().unwrap_or_default().to_string_lossy(),
                target.file_name().unwrap_or_default().to_string_lossy(),
                source.len()
            );
        }
    }
    Ok(report)
}

fn run(opts: Options) -> Result<bool, String> {
    let cache = match opts.cache.clone() {
        Some(dir) => dir,
        None => {
            let found = steam::find_cache_dirs();
            let first = found
                .into_iter()
                .next()
                .ok_or("кэш Garry's Mod не найден, укажите путь вручную")?;
            println!("Найден кэш: {}", first.display());
            first
        }
    };

    if !cache.is_dir() {
        return Err(format!("папка не найдена: {}", cache.display()));
    }

    let report = extract_cache(&cache, &opts).map_err(|e| e.to_string())?;
    println!(
        "Готово: {} скриптов в {}, дубликатов пропущено: {}, ошибок: {}",
        report.extracted,
        opts.output.display(),
        report.duplicates,
        report.failed.len()
    );
    Ok(report.extracted > 0 || report.failed.is_empty())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return ExitCode::SUCCESS;
    }

    let opts = match parse_args(&args) {
        Ok(opts) => opts,
        Err(error) => {
            eprintln!("Ошибка: {error}");
            eprintln!("Подсказка: gmod_lua_extractor --help");
            return ExitCode::FAILURE;
        }
    };

    match run(opts) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Ошибка: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Минимальная реализация SHA-1 (RFC 3174) без внешних зависимостей —
/// используется только для сравнения содержимого распакованных файлов.
struct Sha1;

impl Sha1 {
    fn digest(data: &[u8]) -> [u8; 20] {
        let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

        let mut msg = data.to_vec();
        let bit_len = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        for chunk in msg.chunks_exact(64) {
            let mut w = [0u32; 80];
            for (i, word) in chunk.chunks_exact(4).enumerate() {
                w[i] = u32::from_be_bytes(word.try_into().unwrap());
            }
            for i in 16..80 {
                w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
            }

            let [mut a, mut b, mut c, mut d, mut e] = h;
            for (i, &word) in w.iter().enumerate() {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                    20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                    _ => (b ^ c ^ d, 0xCA62C1D6),
                };
                let tmp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(word);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = tmp;
            }

            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
        }

        let mut out = [0u8; 20];
        for (i, word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(
            content_digest(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            content_digest(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn parse_args_defaults() {
        let opts = parse_args(&[]).unwrap();
        assert!(opts.cache.is_none());
        assert_eq!(opts.output, PathBuf::from("extracted_lua"));
        assert!(opts.dedupe);
    }

    #[test]
    fn parse_args_values() {
        let args: Vec<String> = ["cache/lua", "-o", "out", "--keep-duplicates", "-v"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.cache, Some(PathBuf::from("cache/lua")));
        assert_eq!(opts.output, PathBuf::from("out"));
        assert!(!opts.dedupe);
        assert!(opts.verbose);
    }

    #[test]
    fn parse_args_rejects_unknown() {
        assert!(parse_args(&["--nope".to_string()]).is_err());
    }
}
