//! Декодирование файлов из `garrysmod/cache/lua`.
//!
//! Когда клиент заходит на сервер (или запускает локальную игру), сервер
//! присылает клиентские Lua-файлы, и игра сохраняет их в
//! `garrysmod/cache/lua/*.lua`. Содержимое сжато LZMA в формате
//! `util.Compress`: 5 байт свойств + 8 байт размера (little-endian) + поток
//! `.lzma` (ALONE). Перед сжатыми данными может стоять служебный заголовок —
//! в старых версиях 4 байта CRC32, в новых 32 байта SHA-256, — поэтому декодер
//! пробует несколько смещений и проверяет результат.

use std::fmt;

/// Смещения начала LZMA-потока, известные по разным версиям игры.
const KNOWN_OFFSETS: &[usize] = &[0, 4, 32, 36, 64];
const MAX_SCAN_OFFSET: usize = 128;
const MAX_UNPACKED_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum DecodeError {
    /// Файл не похож на сжатый Lua из кэша.
    NotCacheData,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::NotCacheData => {
                write!(f, "не удалось распаковать LZMA-данные")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Пытается распаковать `.lzma`-поток (ALONE), начинающийся с начала `blob`.
fn try_lzma(blob: &[u8]) -> Option<Vec<u8>> {
    // Байт свойств LZMA = (pb*5 + lp)*9 + lc, максимум 224.
    if blob.len() < 13 || blob[0] >= 225 {
        return None;
    }
    let size = u64::from_le_bytes(blob[5..13].try_into().ok()?);
    let known_size = size != u64::MAX;
    if known_size && size > MAX_UNPACKED_SIZE {
        return None;
    }

    let mut input = blob;
    let mut output = Vec::new();
    lzma_rs::lzma_decompress(&mut input, &mut output).ok()?;

    if known_size && output.len() as u64 != size {
        return None;
    }
    Some(output)
}

/// Эвристика: похоже ли на текст Lua (почти нет управляющих байтов).
fn looks_like_text(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(4096)];
    if sample.is_empty() {
        return true;
    }
    let bad = sample
        .iter()
        .filter(|&&b| b < 9 || (b > 13 && b < 32))
        .count();
    (bad as f64) / (sample.len() as f64) < 0.02
}

fn trim_trailing_nuls(mut data: Vec<u8>) -> Vec<u8> {
    while data.last() == Some(&0) {
        data.pop();
    }
    data
}

/// Возвращает исходный Lua-код из содержимого файла кэша.
pub fn decode_cache_file(raw: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let scan = (0..MAX_SCAN_OFFSET).filter(|o| !KNOWN_OFFSETS.contains(o));
    for offset in KNOWN_OFFSETS.iter().copied().chain(scan) {
        if offset >= raw.len() {
            break;
        }
        if let Some(data) = try_lzma(&raw[offset..]) {
            let data = trim_trailing_nuls(data);
            if !data.is_empty() && looks_like_text(&data) {
                return Ok(data);
            }
        }
    }

    // Некоторые файлы лежат в кэше без сжатия.
    if !raw.is_empty() && looks_like_text(raw) {
        return Ok(trim_trailing_nuls(raw.to_vec()));
    }
    Err(DecodeError::NotCacheData)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &[u8] =
        b"hook.Add(\"HUDPaint\", \"demo\", function() draw.SimpleText(\"hi\") end)\n";

    /// Как util.Compress: 5 байт свойств + 8 байт размера + .lzma-поток.
    fn gmod_compress(data: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        let mut input = data;
        lzma_rs::lzma_compress(&mut input, &mut raw).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&raw[..5]);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&raw[13..]);
        out
    }

    #[test]
    fn decodes_known_offsets() {
        for header_len in [0usize, 4, 32, 36] {
            let mut blob = vec![0xABu8; header_len];
            blob.extend_from_slice(&gmod_compress(SOURCE));
            assert_eq!(
                decode_cache_file(&blob).unwrap(),
                SOURCE,
                "len={header_len}"
            );
        }
    }

    #[test]
    fn decodes_plain_text() {
        assert_eq!(decode_cache_file(SOURCE).unwrap(), SOURCE);
    }

    #[test]
    fn rejects_garbage() {
        let garbage: Vec<u8> = (0u16..1024).map(|i| (i % 256) as u8).collect();
        assert!(decode_cache_file(&garbage).is_err());
    }

    #[test]
    fn trailing_nuls_removed() {
        let mut padded = SOURCE.to_vec();
        padded.extend_from_slice(&[0, 0, 0]);
        let blob = gmod_compress(&padded);
        assert_eq!(decode_cache_file(&blob).unwrap(), SOURCE);
    }
}
