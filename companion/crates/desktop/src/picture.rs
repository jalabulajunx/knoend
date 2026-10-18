//! Pictures for the board's keys: app icons and site icons decoded (PNG, ICO, JPEG, GIF, WebP,
//! SVG) and fitted into a square. Every system.

use std::path::Path;

/// RGBA pixels of `path` (PNG, ICO, JPEG, GIF, WebP or SVG) fitted into `size` x `size`.
pub fn render(path: &Path, size: u32) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    render_bytes(&bytes, path.extension().and_then(|e| e.to_str()) == Some("svg"), size)
}

/// Same, from bytes already read (favicons).
pub fn render_bytes(bytes: &[u8], svg: bool, size: u32) -> Option<Vec<u8>> {
    if svg || bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
        let s = tree.size();
        let scale = size as f32 / s.width().max(s.height());
        let dx = (size as f32 - s.width() * scale) / 2.0;
        let dy = (size as f32 - s.height() * scale) / 2.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, dx, dy), &mut pixmap.as_mut());
        // tiny-skia keeps premultiplied alpha: undo it.
        return Some(pixmap.pixels().iter().flat_map(|p| { let c = p.demultiply(); [c.red(), c.green(), c.blue(), c.alpha()] }).collect());
    }
    let image = image::load_from_memory(bytes).ok()?;
    let fitted = image.resize(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut canvas = image::RgbaImage::new(size, size);
    let x = (size - fitted.width()) / 2;
    let y = (size - fitted.height()) / 2;
    image::imageops::overlay(&mut canvas, &fitted, x as i64, y as i64);
    Some(canvas.into_raw())
}

/// RGBA pixels as PNG bytes.
pub fn png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let image = image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_renders_to_the_requested_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;
        let pixels = render_bytes(svg, true, 32).unwrap();
        assert_eq!(pixels.len(), 32 * 32 * 4);
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }
}
