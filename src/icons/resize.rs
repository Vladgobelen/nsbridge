//! Билинейный ресайз RGBA → RGBA. Достаточно для иконок.

/// Ресайз RGBA-буфера (src_w × src_h) до (dst_w × dst_h).
pub fn resize(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(dst_w * dst_h * 4);
    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = (dx as f32 + 0.5) * scale_x - 0.5;
            let sy = (dy as f32 + 0.5) * scale_y - 0.5;

            let x0 = sx.floor() as isize;
            let y0 = sy.floor() as isize;
            let x1 = x0 + 1;
            let y1 = y0 + 1;

            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let c00 = get_pixel(rgba, src_w, src_h, x0, y0);
            let c10 = get_pixel(rgba, src_w, src_h, x1, y0);
            let c01 = get_pixel(rgba, src_w, src_h, x0, y1);
            let c11 = get_pixel(rgba, src_w, src_h, x1, y1);

            let mut c = [0u8; 4];
            for k in 0..4 {
                let top = c00[k] as f32 * (1.0 - fx) + c10[k] as f32 * fx;
                let bot = c01[k] as f32 * (1.0 - fx) + c11[k] as f32 * fx;
                c[k] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
            }
            out.extend_from_slice(&c);
        }
    }
    out
}

fn get_pixel(rgba: &[u8], w: usize, h: usize, x: isize, y: isize) -> [u8; 4] {
    let x = x.max(0).min(w as isize - 1) as usize;
    let y = y.max(0).min(h as isize - 1) as usize;
    let i = (y * w + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}