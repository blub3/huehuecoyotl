//! Poster scaling (SPEC 7, scan step 5): grids load small pre-scaled JPEGs from disk, never a
//! full-size poster (SPEC 9 performance budgets).

use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use std::io::Cursor;

/// Grid thumbnail width in pixels: covers a 180 px tile at 2× scale.
pub const GRID_WIDTH: u32 = 360;

/// The larger grid tier, role `poster_grid_l`: covers the large cover size (260 px) at 2× scale.
pub const GRID_LARGE_WIDTH: u32 = 540;

/// Decode any supported image and re-encode it as a JPEG no wider than `max_width`.
/// CPU-bound: call from `spawn_blocking`.
/// Issue #12: an image kept as it is (logos, disc art): its file extension and size, or why it
/// cannot be read.
pub fn inspect(data: &[u8]) -> Result<(&'static str, u32, u32), String> {
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let ext = match reader.format() {
        Some(image::ImageFormat::Png) => "png",
        Some(image::ImageFormat::WebP) => "webp",
        Some(image::ImageFormat::Jpeg) => "jpg",
        _ => return Err("not a PNG, JPEG or WebP image".into()),
    };
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    Ok((ext, w, h))
}

pub fn scale_to_jpeg(data: &[u8], max_width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let img = if img.width() > max_width {
        let h = (u64::from(img.height()) * u64::from(max_width) / u64::from(img.width())) as u32;
        img.resize_exact(max_width, h.max(1), image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok((out, rgb.width(), rgb.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_down_keeping_aspect() {
        let img = image::RgbImage::from_fn(1000, 1500, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 90])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (jpg, w, h) = scale_to_jpeg(&png, GRID_WIDTH).unwrap();
        assert_eq!((w, h), (360, 540));
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        assert!(scale_to_jpeg(b"not an image", 100).is_err());
    }
}
