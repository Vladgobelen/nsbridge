//! Извлечение иконок из Windows `.exe` (PE-файлов) и декодирование `.ico`.

use std::io::Cursor;
use std::path::Path;

use icoextract_rs::{IconExtractor, IconSelector};

pub struct DecodedIcon {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub fn extract_from_exe(exe_path: &Path) -> Option<DecodedIcon> {
    let extractor = IconExtractor::from_path(exe_path).ok()?;
    let icon = extractor.get_icon(IconSelector::Index(0)).ok()?;
    let ico_bytes = icon.to_ico_bytes().ok()?;
    decode_ico_bytes(&ico_bytes)
}

pub fn decode_ico_file(path: &Path) -> Option<DecodedIcon> {
    let bytes = std::fs::read(path).ok()?;
    decode_ico_bytes(&bytes)
}

fn decode_ico_bytes(bytes: &[u8]) -> Option<DecodedIcon> {
    use ico::IconDir;

    let dir = IconDir::read(Cursor::new(bytes)).ok()?;

    let mut best: Option<&ico::IconDirEntry> = None;
    let mut best_size = 0u32;
    for entry in dir.entries() {
        let size = (entry.width() as u32) * (entry.height() as u32);
        if size > best_size {
            best_size = size;
            best = Some(entry);
        }
    }

    let entry = best?;
    let image = entry.decode().ok()?;

    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgba = image.rgba_data().to_vec();

    Some(DecodedIcon {
        rgba,
        width: w,
        height: h,
    })
}