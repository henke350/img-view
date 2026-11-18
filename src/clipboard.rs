//! Converting between `image` and egui's clipboard image type.
//!
//! egui (through `arboard`) does the actual Windows clipboard work: Ctrl+V arrives
//! as `Event::PasteImage` and `Context::copy_image` puts an image on the clipboard.

use anyhow::{Context as _, Result};
use egui::ColorImage;
use image::{DynamicImage, RgbaImage};

/// Converts an image to egui's format so it can go on the clipboard.
pub fn to_color_image(image: &DynamicImage) -> ColorImage {
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
}

/// Converts an image pasted from the clipboard into an editable image.
pub fn from_color_image(image: &ColorImage) -> Result<DynamicImage> {
    let [width, height] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let rgba = RgbaImage::from_raw(
        u32::try_from(width).context("pasted image is too wide")?,
        u32::try_from(height).context("pasted image is too tall")?,
        bytes,
    )
    .context("pasted image data is incomplete")?;
    Ok(DynamicImage::ImageRgba8(rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sample() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(3, 2, |x, y| {
            Rgba([u8::try_from(x * 80).unwrap(), u8::try_from(y * 100).unwrap(), 7, 255])
        }))
    }

    #[test]
    fn to_color_image_should_keep_size() {
        assert_eq!(to_color_image(&sample()).size, [3, 2]);
    }

    #[test]
    fn round_trip_should_keep_opaque_pixels() {
        let back = from_color_image(&to_color_image(&sample())).unwrap();
        assert_eq!(back.to_rgba8(), sample().to_rgba8());
    }
}
