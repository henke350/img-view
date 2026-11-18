//! Pure image-editing operations: crop and resize.
//!
//! Kept free of any UI code so they can be unit tested.

use anyhow::{Result, bail};
use image::DynamicImage;
use image::imageops::FilterType;

/// A rectangle in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Crops `img` to `rect`, clamping the rectangle to the image bounds.
///
/// Fails if the clamped rectangle is empty.
pub fn crop(img: &DynamicImage, rect: PixelRect) -> Result<DynamicImage> {
    let x = rect.x.min(img.width());
    let y = rect.y.min(img.height());
    let width = rect.width.min(img.width() - x);
    let height = rect.height.min(img.height() - y);
    if width == 0 || height == 0 {
        bail!("crop area is empty");
    }
    Ok(img.crop_imm(x, y, width, height))
}

/// Resizes `img` to exactly `width` x `height` using a high-quality filter.
pub fn resize(img: &DynamicImage, width: u32, height: u32) -> Result<DynamicImage> {
    if width == 0 || height == 0 {
        bail!("width and height must be at least 1 pixel");
    }
    Ok(img.resize_exact(width, height, FilterType::Lanczos3))
}

/// Height that keeps the aspect ratio of `orig_width` x `orig_height` at `new_width`.
pub fn proportional_height(orig_width: u32, orig_height: u32, new_width: u32) -> u32 {
    if orig_width == 0 {
        return 0;
    }
    let h = (u64::from(new_width) * u64::from(orig_height) + u64::from(orig_width) / 2)
        / u64::from(orig_width);
    u32::try_from(h).unwrap_or(u32::MAX).max(1)
}

/// Width that keeps the aspect ratio of `orig_width` x `orig_height` at `new_height`.
pub fn proportional_width(orig_width: u32, orig_height: u32, new_height: u32) -> u32 {
    proportional_height(orig_height, orig_width, new_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(width: u32, height: u32) -> DynamicImage {
        DynamicImage::new_rgba8(width, height)
    }

    #[test]
    fn crop_should_return_requested_size_when_inside_bounds() {
        let rect = PixelRect { x: 10, y: 20, width: 30, height: 40 };
        let out = crop(&blank(100, 100), rect).unwrap();
        assert_eq!((out.width(), out.height()), (30, 40));
    }

    #[test]
    fn crop_should_clamp_when_rect_exceeds_bounds() {
        let rect = PixelRect { x: 80, y: 90, width: 50, height: 50 };
        let out = crop(&blank(100, 100), rect).unwrap();
        assert_eq!((out.width(), out.height()), (20, 10));
    }

    #[test]
    fn crop_should_fail_when_rect_is_outside_image() {
        let rect = PixelRect { x: 200, y: 0, width: 10, height: 10 };
        assert!(crop(&blank(100, 100), rect).is_err());
    }

    #[test]
    fn resize_should_return_exact_size() {
        let out = resize(&blank(100, 50), 33, 77).unwrap();
        assert_eq!((out.width(), out.height()), (33, 77));
    }

    #[test]
    fn resize_should_fail_when_size_is_zero() {
        assert!(resize(&blank(100, 50), 0, 10).is_err());
    }

    #[test]
    fn proportional_height_should_keep_aspect_ratio() {
        assert_eq!(proportional_height(4000, 3000, 800), 600);
    }

    #[test]
    fn proportional_height_should_never_be_zero() {
        assert_eq!(proportional_height(4000, 10, 1), 1);
    }

    #[test]
    fn proportional_width_should_keep_aspect_ratio() {
        assert_eq!(proportional_width(1920, 1080, 540), 960);
    }
}
