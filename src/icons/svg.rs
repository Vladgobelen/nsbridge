//! Декодирование SVG в RGBA-буфер с помощью usvg + tiny-skia.

use std::path::Path;

use tiny_skia::Pixmap;
use usvg::{Options, Tree};

pub struct DecodedSvg {
    pub rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub fn decode(path: &Path, size: u32) -> Option<DecodedSvg> {
    let svg_data = std::fs::read_to_string(path).ok()?;

    let opt = Options::default();
    let tree = Tree::from_str(&svg_data, &opt).ok()?;

    let svg_size = tree.size();
    let svg_w = svg_size.width();
    let svg_h = svg_size.height();

    let scale = (size as f32 / svg_w.max(svg_h)).min(1.0);
    let out_w = (svg_w * scale).ceil().max(1.0) as u32;
    let out_h = (svg_h * scale).ceil().max(1.0) as u32;

    let mut pixmap = Pixmap::new(out_w, out_h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for chunk in pixmap.data().chunks(4) {
        let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        if a > 0 {
            let inv_a = 255.0 / a as f32;
            rgba.push((r as f32 * inv_a).min(255.0) as u8);
            rgba.push((g as f32 * inv_a).min(255.0) as u8);
            rgba.push((b as f32 * inv_a).min(255.0) as u8);
            rgba.push(a);
        } else {
            rgba.push(0);
            rgba.push(0);
            rgba.push(0);
            rgba.push(0);
        }
    }

    Some(DecodedSvg {
        rgba,
        width: out_w as usize,
        height: out_h as usize,
    })
}