//! Printing through the classic Windows GDI print API.
//!
//! The top half is plain maths (where the image goes on the page) and is unit tested.
//! The bottom half talks to Windows; every `unsafe` block in the app lives here.

use std::path::Path;

use anyhow::{Result, bail};
use image::DynamicImage;
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HALFTONE, HDC,
    HORZRES, LOGPIXELSX, LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, SRCCOPY,
    SetBrushOrgEx, SetStretchBltMode, StretchDIBits, VERTRES,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Controls::Dialogs::{
    DEVNAMES, PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC, PD_RETURNDEFAULT, PD_USEDEVMODECOPIESANDCOLLATE,
    PRINTDLGEX_FLAGS, PRINTDLGW, PrintDlgW,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::core::PCWSTR;

const CM_PER_INCH: f32 = 2.54;

/// How big the image comes out on paper. The aspect ratio is always kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintMode {
    /// As large as fits in the printable area (small images are enlarged).
    FitToPage,
    /// Image pixels at `PrintOptions::image_dpi`.
    Original,
    /// A percentage of the original size.
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrintOptions {
    pub mode: PrintMode,
    pub scale_percent: f32,
    /// How many image pixels make one inch at original size.
    pub image_dpi: f32,
}

impl Default for PrintOptions {
    fn default() -> Self {
        // 96 dpi is what Windows assumes for a screen, so "original size" matches 100% on screen.
        Self { mode: PrintMode::FitToPage, scale_percent: 100.0, image_dpi: 96.0 }
    }
}

/// A printer page, in printer dots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// The whole sheet.
    pub paper: (i32, i32),
    /// Where the printable area starts on the sheet.
    pub offset: (i32, i32),
    /// The area the printer can actually print on.
    pub printable: (i32, i32),
    /// Printer dots per inch, horizontally and vertically.
    pub dpi: (i32, i32),
}

/// Where the image lands, in dots relative to the printable area's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Placement {
    /// False if part of the image falls outside the printable area and will be cut off.
    pub fn fits(&self, page: &Page) -> bool {
        self.x >= 0 && self.y >= 0 && self.x + self.width <= page.printable.0 && self.y + self.height <= page.printable.1
    }

    /// Printed size in centimetres.
    pub fn size_cm(&self, page: &Page) -> (f32, f32) {
        (
            self.width as f32 / page.dpi.0 as f32 * CM_PER_INCH,
            self.height as f32 / page.dpi.1 as f32 * CM_PER_INCH,
        )
    }
}

/// Centres an image of `image` pixels in the printable area, sized according to `options`.
pub fn layout(image: (u32, u32), options: &PrintOptions, page: &Page) -> Placement {
    let (img_w, img_h) = (image.0.max(1) as f32, image.1.max(1) as f32);
    let (dpi_x, dpi_y) = (page.dpi.0 as f32, page.dpi.1 as f32);
    // Work in inches so printers with non-square dots still keep the image's shape.
    let width_in = match options.mode {
        PrintMode::FitToPage => {
            let (avail_w, avail_h) = (page.printable.0 as f32 / dpi_x, page.printable.1 as f32 / dpi_y);
            avail_w.min(avail_h * img_w / img_h)
        }
        PrintMode::Original => img_w / options.image_dpi.max(1.0),
        PrintMode::Scale => img_w / options.image_dpi.max(1.0) * options.scale_percent.max(0.1) / 100.0,
    };
    let height_in = width_in * img_h / img_w;
    let width = (width_in * dpi_x).round() as i32;
    let height = (height_in * dpi_y).round() as i32;
    Placement { x: (page.printable.0 - width) / 2, y: (page.printable.1 - height) / 2, width, height }
}

/// The image as a bottom-up 24-bit BGR bitmap, the layout GDI expects, with transparent
/// areas flattened onto white paper. Each row is padded to a multiple of 4 bytes.
pub fn dib_rows(image: &DynamicImage) -> Vec<u8> {
    let rgba = image.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let stride = (w * 3).next_multiple_of(4);
    let mut out = vec![0u8; stride * h];
    for (y, row) in rgba.rows().enumerate() {
        let start = (h - 1 - y) * stride;
        for (x, px) in row.enumerate() {
            let [r, g, b, a] = px.0;
            let over_white = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
            let i = start + x * 3;
            out[i] = over_white(b);
            out[i + 1] = over_white(g);
            out[i + 2] = over_white(r);
        }
    }
    out
}

/// An open connection to a printer. Released when dropped.
pub struct Printer {
    hdc: HDC,
    pub name: String,
}

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: `hdc` came from PrintDlgW or CreateDCW and is deleted exactly once, here.
        unsafe {
            let _ = DeleteDC(self.hdc);
        }
    }
}

impl Printer {
    /// The default printer, without showing any dialog. `None` if no printer is installed.
    pub fn default_printer() -> Option<Self> {
        print_dialog(PD_RETURNDEFAULT | PD_RETURNDC)
    }

    /// Shows the Windows print dialog. `None` if the user cancelled.
    pub fn choose() -> Option<Self> {
        print_dialog(PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE)
    }

    /// Opens a printer by its name, as shown in Windows settings.
    #[cfg(test)]
    pub fn by_name(name: &str) -> Option<Self> {
        let wide = wide(name);
        // SAFETY: both strings are nul-terminated UTF-16 that outlive the call.
        let hdc = unsafe { windows::Win32::Graphics::Gdi::CreateDCW(windows::core::w!("WINSPOOL"), PCWSTR(wide.as_ptr()), PCWSTR::null(), None) };
        (!hdc.is_invalid()).then(|| Self { hdc, name: name.to_owned() })
    }

    pub fn page(&self) -> Page {
        // SAFETY: `hdc` is a valid printer device context for as long as `self` lives.
        let cap = |index| unsafe { GetDeviceCaps(Some(self.hdc), index) };
        Page {
            paper: (cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT)),
            offset: (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY)),
            printable: (cap(HORZRES), cap(VERTRES)),
            dpi: (cap(LOGPIXELSX).max(1), cap(LOGPIXELSY).max(1)),
        }
    }

    /// Prints `image` as one page. With `output`, the printer writes to that file instead
    /// (used to test with "Microsoft Print to PDF").
    pub fn print(&self, image: &DynamicImage, options: &PrintOptions, title: &str, output: Option<&Path>) -> Result<()> {
        let page = self.page();
        let place = layout((image.width(), image.height()), options, &page);
        let bits = dib_rows(image);
        let (Ok(width), Ok(height)) = (i32::try_from(image.width()), i32::try_from(image.height())) else {
            bail!("image is too large to print");
        };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: height, // positive: rows run bottom-up, matching `dib_rows`
                biPlanes: 1,
                biBitCount: 24,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let title = wide(title);
        let output = output.map(|p| wide(&p.to_string_lossy()));
        let doc = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(title.as_ptr()),
            lpszOutput: output.as_ref().map_or(PCWSTR::null(), |o| PCWSTR(o.as_ptr())),
            ..Default::default()
        };

        // SAFETY: `hdc` is valid; `doc`, `info` and `bits` (sized by `dib_rows` for exactly
        // this width and height) all outlive the calls that read them.
        unsafe {
            if StartDocW(self.hdc, &doc) <= 0 {
                bail!("the printer did not accept the job");
            }
            let drawn = StartPage(self.hdc) > 0 && {
                SetStretchBltMode(self.hdc, HALFTONE);
                let _ = SetBrushOrgEx(self.hdc, 0, 0, None);
                let lines = StretchDIBits(
                    self.hdc,
                    place.x,
                    place.y,
                    place.width,
                    place.height,
                    0,
                    0,
                    width,
                    height,
                    Some(bits.as_ptr().cast()),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                lines > 0 && EndPage(self.hdc) > 0
            };
            if !drawn {
                AbortDoc(self.hdc);
                bail!("could not draw the image on the printer page");
            }
            if EndDoc(self.hdc) <= 0 {
                bail!("the printer did not finish the job");
            }
        }
        Ok(())
    }
}

fn print_dialog(flags: PRINTDLGEX_FLAGS) -> Option<Printer> {
    let mut dlg = PRINTDLGW {
        lStructSize: size_of::<PRINTDLGW>() as u32,
        // SAFETY: no preconditions; the dialog is owned by whichever window is in front (ours).
        hwndOwner: unsafe { GetForegroundWindow() },
        Flags: flags,
        nCopies: 1,
        ..Default::default()
    };
    // SAFETY: `dlg` is a correctly sized, initialised PRINTDLGW.
    let ok = unsafe { PrintDlgW(&mut dlg) }.as_bool();
    let name = device_name(dlg.hDevNames);
    // SAFETY: the dialog allocated these handles for us to free; null handles are skipped.
    unsafe {
        for handle in [dlg.hDevMode, dlg.hDevNames] {
            if !handle.is_invalid() {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
    (ok && !dlg.hDC.is_invalid()).then(|| Printer { hdc: dlg.hDC, name })
}

/// Reads the printer name out of the dialog's DEVNAMES block.
fn device_name(handle: HGLOBAL) -> String {
    if handle.is_invalid() {
        return String::new();
    }
    // SAFETY: DEVNAMES is a header of u16 offsets (in characters) into the same block,
    // each pointing at a nul-terminated UTF-16 string. The block is unlocked before return.
    unsafe {
        let base = GlobalLock(handle);
        if base.is_null() {
            return String::new();
        }
        let names = &*base.cast::<DEVNAMES>();
        let name = PCWSTR(base.cast::<u16>().add(usize::from(names.wDeviceOffset))).to_string().unwrap_or_default();
        let _ = GlobalUnlock(handle);
        name
    }
}

/// A nul-terminated UTF-16 copy of `s`, as Windows APIs expect.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A4 at 600 dpi with a small unprintable margin.
    const A4: Page = Page { paper: (4960, 7016), offset: (100, 100), printable: (4760, 6816), dpi: (600, 600) };

    fn options(mode: PrintMode) -> PrintOptions {
        PrintOptions { mode, ..Default::default() }
    }

    #[test]
    fn fit_to_page_should_fill_width_of_landscape_image() {
        let place = layout((1600, 1000), &options(PrintMode::FitToPage), &A4);
        assert_eq!((place.width, place.height), (4760, 2975));
    }

    #[test]
    fn fit_to_page_should_centre_vertically() {
        let place = layout((1600, 1000), &options(PrintMode::FitToPage), &A4);
        assert_eq!((place.x, place.y), (0, (6816 - 2975) / 2));
    }

    #[test]
    fn fit_to_page_should_enlarge_small_image() {
        let place = layout((10, 20), &options(PrintMode::FitToPage), &A4);
        assert_eq!(place.height, 6816);
    }

    #[test]
    fn original_size_should_use_image_dpi() {
        // 960 px at 96 dpi = 10 inches = 6000 dots at 600 dpi.
        let place = layout((960, 480), &options(PrintMode::Original), &A4);
        assert_eq!((place.width, place.height), (6000, 3000));
    }

    #[test]
    fn original_size_should_report_cut_off_when_larger_than_page() {
        let place = layout((960, 480), &options(PrintMode::Original), &A4);
        assert!(!place.fits(&A4));
    }

    #[test]
    fn scale_should_keep_aspect_ratio() {
        let opts = PrintOptions { mode: PrintMode::Scale, scale_percent: 50.0, image_dpi: 96.0 };
        let place = layout((960, 480), &opts, &A4);
        assert_eq!((place.width, place.height), (3000, 1500));
    }

    #[test]
    fn size_cm_should_convert_dots() {
        let place = Placement { x: 0, y: 0, width: 600, height: 1200 };
        assert_eq!(place.size_cm(&A4), (2.54, 5.08));
    }

    #[test]
    fn dib_rows_should_store_bottom_row_first_as_bgr() {
        let mut img = image::RgbaImage::new(1, 2);
        img.put_pixel(0, 1, image::Rgba([10, 20, 30, 255]));
        let bits = dib_rows(&DynamicImage::ImageRgba8(img));
        assert_eq!(&bits[..3], &[30, 20, 10]);
    }

    #[test]
    fn dib_rows_should_pad_rows_to_four_bytes() {
        // 1 pixel = 3 bytes, padded to 4; two rows.
        assert_eq!(dib_rows(&DynamicImage::new_rgb8(1, 2)).len(), 8);
    }

    #[test]
    fn dib_rows_should_print_transparent_pixels_as_white() {
        let bits = dib_rows(&DynamicImage::new_rgba8(1, 1));
        assert_eq!(&bits[..3], &[255, 255, 255]);
    }

    /// Prints a real page through "Microsoft Print to PDF" into a temp file.
    /// Run with: cargo test print_to_pdf -- --ignored
    #[test]
    #[ignore = "needs the Microsoft Print to PDF printer; writes a file"]
    fn print_to_pdf_should_write_a_file() {
        let printer = Printer::by_name("Microsoft Print to PDF").expect("printer installed");
        let path = std::env::temp_dir().join("img-view-print-test.pdf");
        let _ = std::fs::remove_file(&path);
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(1600, 1000, |x, y| {
            image::Rgb([(x / 7) as u8, (y / 4) as u8, 160])
        }));
        printer.print(&img, &PrintOptions::default(), "img-view test", Some(&path)).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
    }
}
