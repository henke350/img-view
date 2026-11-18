//! Look and feel: neutral greys so the chrome never tints how a photo is judged,
//! and one accent colour reserved for the selection.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// Toolbar and status bar background.
pub const CHROME: Color32 = Color32::from_rgb(0x23, 0x23, 0x23);
/// Background around the image: a neutral mid-dark grey, slightly lighter than the chrome.
pub const WELL: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
pub const MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
const BUTTON: Color32 = Color32::from_rgb(0x2F, 0x2F, 0x2F);
const BUTTON_HOVER: Color32 = Color32::from_rgb(0x3D, 0x3D, 0x3D);
const BUTTON_DOWN: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const CONTROL: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const CONTROL_HOVER: Color32 = Color32::from_rgb(0x5C, 0x5C, 0x5C);
const HAIRLINE: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
/// Selection yellow: used only for the selection outline and the Crop button it enables.
pub const MARQUEE: Color32 = Color32::from_rgb(0xF0, 0xC4, 0x19);
/// Text drawn on top of `MARQUEE`.
pub const ON_MARQUEE: Color32 = Color32::from_rgb(0x1A, 0x16, 0x05);

/// The Windows UI font, so the viewer feels native. Falls back to egui's font if missing.
const SYSTEM_FONT: &str = r"C:\Windows\Fonts\segoeui.ttf";
/// Windows 11's icon font, then Windows 10's, which shares most code points.
const ICON_FONTS: [&str; 2] = [r"C:\Windows\Fonts\SegoeIcons.ttf", r"C:\Windows\Fonts\segmdl2.ttf"];
const ICON_FAMILY: &str = "icons";

/// Font for toolbar icons. Only valid when [`apply`] returned `true`.
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICON_FAMILY.into()))
}

/// Styles the app. Returns whether an icon font was found.
pub fn apply(ctx: &egui::Context) -> bool {
    let has_icons = install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.item_spacing = Vec2::new(4.0, 4.0);

        let v = &mut style.visuals;
        v.panel_fill = CHROME;
        v.window_fill = CHROME;
        v.window_stroke = Stroke::new(1.0, HAIRLINE);
        v.override_text_color = None;
        v.selection.bg_fill = MARQUEE.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, MARQUEE);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
        // Buttons use `weak_bg_fill`; radio buttons, checkboxes and number fields use
        // `bg_fill`, which needs more contrast to be visible on the dark chrome.
        for (w, fill, control) in [
            (&mut v.widgets.inactive, BUTTON, CONTROL),
            (&mut v.widgets.hovered, BUTTON_HOVER, CONTROL_HOVER),
            (&mut v.widgets.active, BUTTON_DOWN, CONTROL_HOVER),
        ] {
            w.weak_bg_fill = fill;
            w.bg_fill = control;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.0, TEXT);
            w.corner_radius = CornerRadius::same(4);
            w.expansion = 0.0;
        }
    });
    has_icons
}

fn install_fonts(ctx: &egui::Context) -> bool {
    let mut fonts = FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(SYSTEM_FONT) {
        fonts.font_data.insert("segoe-ui".to_owned(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "segoe-ui".to_owned());
        }
    }
    let icons = ICON_FONTS.iter().find_map(|path| std::fs::read(path).ok());
    let has_icons = icons.is_some();
    if let Some(bytes) = icons {
        fonts.font_data.insert(ICON_FAMILY.to_owned(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.insert(FontFamily::Name(ICON_FAMILY.into()), vec![ICON_FAMILY.to_owned()]);
    }
    ctx.set_fonts(fonts);
    has_icons
}
