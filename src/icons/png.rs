//! Обертка над крейтом `png` для чтения изображений.
//! Приводит любой PNG (RGB, RGBA, Grayscale, Palette) к сырому RGBA8 буферу.
//!
//! Для `Indexed` читаем `tRNS` (прозрачность палитры). Раньше альфа
//! всегда была 255, и прозрачные иконки (в палитровом формате) получали
//! чёрный/непрозрачный фон.

use std::io::BufReader;
use std::path::Path;

/// Результат декодирования PNG: (RGBA-байты, ширина, высота).
pub struct DecodedPng {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

/// Прочитать PNG-файл и вернуть RGBA-буфер.
pub fn decode(path: &Path) -> Option<DecodedPng> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let decoder = png::Decoder::new(reader);

    let mut reader = decoder.read_info().ok()?;

    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let raw = &buf[..info.buffer_size()];

    let (w, h) = (info.width as usize, info.height as usize);

    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),

        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(w * h * 4);
            for chunk in raw.chunks(3) {
                out.extend_from_slice(chunk);
                out.push(255);
            }
            out
        }

        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(w * h * 4);
            for &g in raw {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }

        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(w * h * 4);
            for chunk in raw.chunks(2) {
                let (g, a) = (chunk[0], chunk[1]);
                out.extend_from_slice(&[g, g, g, a]);
            }
            out
        }

        png::ColorType::Indexed => {
            let palette = reader.info().palette.as_ref()?;
            let trns = reader.info().trns.clone();

            let mut out = Vec::with_capacity(w * h * 4);
            for &idx in raw {
                let i = (idx as usize) * 3;
                if i + 2 < palette.len() {
                    let r = palette[i];
                    let g = palette[i + 1];
                    let b = palette[i + 2];
                    let a = match &trns {
                        Some(t) => *t.get(idx as usize).unwrap_or(&255),
                        None => 255,
                    };
                    out.extend_from_slice(&[r, g, b, a]);
                } else {
                    out.extend_from_slice(&[0, 0, 0, 255]);
                }
            }
            out
        }
    };

    Some(DecodedPng {
        rgba,
        width: w,
        height: h,
    })
}