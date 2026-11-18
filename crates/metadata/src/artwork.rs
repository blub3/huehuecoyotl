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

/// Issue #82: the Now Playing background for a cover: the cover's dominant colour (pixels
/// weighted by how colourful they are), with its hue kept, its saturation capped at 0.55 and its
/// lightness set to 0.17, so light text on it always keeps a contrast of at least 4.5:1.
/// "#rrggbb". CPU-bound but small: the cover is scaled to 24 × 24 first.
pub fn tint(data: &[u8]) -> Result<String, String> {
    let img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .resize_exact(24, 24, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let (mut r, mut g, mut b, mut wsum) = (0f64, 0f64, 0f64, 0f64);
    for p in img.pixels() {
        let (pr, pg, pb) = (
            f64::from(p[0]) / 255.0,
            f64::from(p[1]) / 255.0,
            f64::from(p[2]) / 255.0,
        );
        let (_, s, l) = hsl(pr, pg, pb);
        // Colourful, mid-light pixels count most; grey and near-black or white ones a little.
        let w = s * (1.0 - (2.0 * l - 1.0).abs()) + 0.05;
        r += pr * w;
        g += pg * w;
        b += pb * w;
        wsum += w;
    }
    let (h, s, _) = hsl(r / wsum, g / wsum, b / wsum);
    let (r, g, b) = rgb(h, s.min(0.55), 0.17);
    Ok(format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8
    ))
}

fn hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-9 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, s, l)
}

fn rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tint_tests {
    use super::tint;

    fn png(rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 64, image::Rgb(rgb)))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// WCAG contrast of the app's text colour (#eceef1) on a background.
    fn contrast(hex: &str) -> f64 {
        let lum = |h: &str| {
            let c = |i: usize| {
                let v = f64::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap()) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5)
        };
        (lum("#eceef1") + 0.05) / (lum(hex) + 0.05)
    }

    #[test]
    fn tints_keep_the_hue_and_the_contrast() {
        for (cover, hue) in [
            ([240, 220, 40], "yellow"),
            ([20, 60, 200], "blue"),
            ([250, 250, 250], "white"),
            ([5, 5, 5], "black"),
        ] {
            let t = tint(&png(cover)).unwrap();
            assert!(contrast(&t) >= 4.5, "{hue}: {t} has {:.1}:1", contrast(&t));
        }
        // The hue survives: a blue cover gives a blue-ish tint, a yellow one a yellow-ish tint.
        let blue = tint(&png([20, 60, 200])).unwrap();
        assert!(blue[5..7] > blue[1..3], "{blue}");
        let yellow = tint(&png([240, 220, 40])).unwrap();
        assert!(yellow[1..3] > yellow[5..7], "{yellow}");
        assert!(tint(b"not an image").is_err());
    }
}
