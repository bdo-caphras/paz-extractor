//! Optional asset conversion: DDS textures -> PNG.

use std::io::Cursor;

/// Try to convert `data` (identified by `vpath`'s extension) to PNG bytes.
/// Returns `None` if the file is not a convertible image.
pub fn try_convert(vpath: &str, data: &[u8]) -> Option<Vec<u8>> {
    let ext = vpath.rsplit('.').next().unwrap_or("").to_lowercase();
    if ext != "dds" {
        return None;
    }
    dds_to_png(data)
}

/// Decode a DDS blob and re-encode it as PNG.
fn dds_to_png(data: &[u8]) -> Option<Vec<u8>> {
    let dds = dds::DDS::decode(&mut Cursor::new(data)).ok()?;
    let layer = dds.layers.into_iter().next()?;

    let width = dds.header.width;
    let height = dds.header.height;
    if width == 0 || height == 0 {
        return None;
    }

    // dds-rs yields RGBA pixels.
    let mut buf = Vec::with_capacity((width * height * 4) as usize);
    for px in &layer {
        buf.push(px.r);
        buf.push(px.g);
        buf.push(px.b);
        buf.push(px.a);
    }

    let img = image::RgbaImage::from_raw(width, height, buf)?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}
