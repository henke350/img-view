//! The viewer window: shows one image, browses its folder, crops and resizes.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use egui::{Color32, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke, Vec2};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::edit::{self, PixelRect};
use crate::{clipboard, folder};

const MAX_UNDO: usize = 10;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::O);
const SAVE_AS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const CROP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const RESIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
// Only shown in menus: egui handles these keys itself and sends clipboard events.
const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);

/// The image currently open.
struct Document {
    /// `None` for an image pasted from the clipboard that hasn't been saved yet.
    path: Option<PathBuf>,
    image: DynamicImage,
    modified: bool,
}

/// A drag selection, stored in image pixel coordinates so it survives window resizes.
#[derive(Clone, Copy)]
struct Selection {
    start: Pos2,
    end: Pos2,
}

impl Selection {
    fn to_pixel_rect(self) -> PixelRect {
        let min = self.start.min(self.end);
        let max = self.start.max(self.end);
        // Coordinates are already clamped to the image, so they are non-negative.
        let (x, y) = (min.x.round() as u32, min.y.round() as u32);
        PixelRect {
            x,
            y,
            width: (max.x.round() as u32).saturating_sub(x),
            height: (max.y.round() as u32).saturating_sub(y),
        }
    }
}

struct ResizeDialog {
    width: u32,
    height: u32,
    keep_aspect: bool,
}

enum Action {
    Open,
    SaveAs,
    Undo,
    Crop,
    ShowResize,
    Step(isize),
    Copy,
    /// Ask egui to read the clipboard; the result arrives as a paste event.
    RequestPaste,
    PasteImage(Arc<egui::ColorImage>),
    PasteText(String),
}

pub struct ViewerApp {
    doc: Option<Document>,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    folder: Vec<PathBuf>,
    selection: Option<Selection>,
    resize_dialog: Option<ResizeDialog>,
    undo: Vec<DynamicImage>,
    status: String,
}

impl ViewerApp {
    pub fn new(start_file: Option<PathBuf>) -> Self {
        let mut app = Self {
            doc: None,
            texture: None,
            texture_dirty: false,
            folder: Vec::new(),
            selection: None,
            resize_dialog: None,
            undo: Vec::new(),
            status: "Press O to open an image, or drop one here.".to_owned(),
        };
        if let Some(path) = start_file {
            app.open(&path);
        }
        app
    }

    fn open(&mut self, path: &Path) {
        match load_image(path) {
            Ok(image) => {
                if self.folder.iter().all(|p| p != path) {
                    self.folder = folder::images_beside(path);
                }
                self.set_document(Some(path.to_path_buf()), image, false);
                self.status.clear();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn set_document(&mut self, path: Option<PathBuf>, image: DynamicImage, modified: bool) {
        self.doc = Some(Document { path, image, modified });
        self.texture_dirty = true;
        self.selection = None;
        self.undo.clear();
    }

    /// Copies the selection, or the whole image if nothing is selected.
    fn copy(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else { return };
        let copied = match self.selection {
            Some(sel) => edit::crop(&doc.image, sel.to_pixel_rect()),
            None => Ok(doc.image.clone()),
        };
        match copied {
            Ok(image) => {
                self.status = format!("Copied {} × {}.", image.width(), image.height());
                ctx.copy_image(clipboard::to_color_image(&image));
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn paste_image(&mut self, pasted: &egui::ColorImage) {
        match clipboard::from_color_image(pasted) {
            Ok(image) => {
                self.folder.clear();
                self.set_document(None, image, true);
                self.status = "Pasted from clipboard. Ctrl+S to save.".to_owned();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Text on the clipboard is accepted if it is the path of an image file.
    fn paste_text(&mut self, text: &str) {
        let path = PathBuf::from(text.trim().trim_matches('"'));
        if path.is_file() && folder::is_supported(&path) {
            self.open(&path);
        } else {
            self.status = "The clipboard has no image.".to_owned();
        }
    }

    fn open_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter(
            "Images",
            &["jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "ico", "tga", "qoi", "pnm", "hdr", "exr"],
        );
        if let Some(dir) = self.doc.as_ref().and_then(|d| d.path.as_deref()?.parent()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open(&path);
        }
    }

    fn save_as(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        let mut dialog = rfd::FileDialog::new()
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("PNG", &["png"])
            .add_filter("WebP", &["webp"])
            .add_filter("BMP", &["bmp"])
            .add_filter("TIFF", &["tif", "tiff"]);
        match &doc.path {
            Some(current) => {
                if let Some(dir) = current.parent() {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(name) = current.file_name() {
                    dialog = dialog.set_file_name(name.to_string_lossy());
                }
            }
            None => dialog = dialog.set_file_name("pasted.png"),
        }
        let Some(path) = dialog.save_file() else { return };
        match save_image(&doc.image, &path) {
            Ok(()) => {
                self.folder = folder::images_beside(&path);
                doc.path = Some(path);
                doc.modified = false;
                self.status = "Saved.".to_owned();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Replaces the current image with the result of `op`, keeping an undo copy.
    fn apply(&mut self, op: impl FnOnce(&DynamicImage) -> Result<DynamicImage>) {
        let Some(doc) = &mut self.doc else { return };
        match op(&doc.image) {
            Ok(new_image) => {
                let old = std::mem::replace(&mut doc.image, new_image);
                if self.undo.len() == MAX_UNDO {
                    self.undo.remove(0);
                }
                self.undo.push(old);
                doc.modified = true;
                self.texture_dirty = true;
                self.selection = None;
                self.status.clear();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn crop_to_selection(&mut self) {
        match self.selection {
            Some(sel) => self.apply(|img| edit::crop(img, sel.to_pixel_rect())),
            None => self.status = "Drag over the image to select an area first.".to_owned(),
        }
    }

    fn undo(&mut self) {
        if let (Some(doc), Some(previous)) = (&mut self.doc, self.undo.pop()) {
            doc.image = previous;
            doc.modified = !self.undo.is_empty();
            self.texture_dirty = true;
            self.selection = None;
        }
    }

    fn step(&mut self, step: isize) {
        let Some(current) = self.doc.as_ref().and_then(|d| d.path.as_deref()) else { return };
        if let Some(next) = folder::neighbor(&self.folder, current, step).map(Path::to_path_buf) {
            self.open(&next);
        }
    }

    fn show_resize_dialog(&mut self) {
        if let Some(doc) = &self.doc {
            self.resize_dialog = Some(ResizeDialog {
                width: doc.image.width(),
                height: doc.image.height(),
                keep_aspect: true,
            });
        }
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Copy => self.copy(ctx),
            Action::RequestPaste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            Action::PasteImage(image) => self.paste_image(&image),
            Action::PasteText(text) => self.paste_text(&text),
            Action::Open => self.open_dialog(),
            Action::SaveAs => self.save_as(),
            Action::Undo => self.undo(),
            Action::Crop => self.crop_to_selection(),
            Action::ShowResize => self.show_resize_dialog(),
            Action::Step(step) => self.step(step),
        }
    }

    fn keyboard_action(&self, ctx: &egui::Context) -> Option<Action> {
        // Don't steal keys while the user is typing into the resize dialog.
        if self.resize_dialog.is_some() {
            return None;
        }
        ctx.input_mut(|i| {
            // egui turns Ctrl+C / Ctrl+V into these events itself.
            let clipboard_event = i.events.iter().find_map(|e| match e {
                egui::Event::Copy => Some(Action::Copy),
                egui::Event::PasteImage(image) => Some(Action::PasteImage(Arc::clone(image))),
                egui::Event::Paste(text) => Some(Action::PasteText(text.clone())),
                _ => None,
            });
            if clipboard_event.is_some() {
                clipboard_event
            } else if i.consume_shortcut(&SAVE_AS) {
                Some(Action::SaveAs)
            } else if i.consume_shortcut(&UNDO) {
                Some(Action::Undo)
            } else if i.consume_shortcut(&CROP) {
                Some(Action::Crop)
            } else if i.consume_shortcut(&RESIZE) {
                Some(Action::ShowResize)
            } else if i.consume_shortcut(&OPEN) {
                Some(Action::Open)
            } else if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown) || i.key_pressed(Key::Space) {
                Some(Action::Step(1))
            } else if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp) || i.key_pressed(Key::Backspace) {
                Some(Action::Step(-1))
            } else {
                None
            }
        })
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        let has_doc = self.doc.is_some();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("Open…").shortcut_text("O")).clicked() {
                    action = Some(Action::Open);
                }
                let save = egui::Button::new("Save as…").shortcut_text(ui.ctx().format_shortcut(&SAVE_AS));
                if ui.add_enabled(has_doc, save).clicked() {
                    action = Some(Action::SaveAs);
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = egui::Button::new("Undo").shortcut_text(ui.ctx().format_shortcut(&UNDO));
                if ui.add_enabled(!self.undo.is_empty(), undo).clicked() {
                    action = Some(Action::Undo);
                }
                ui.separator();
                let copy_label = if self.selection.is_some() { "Copy selection" } else { "Copy" };
                let copy = egui::Button::new(copy_label).shortcut_text(ui.ctx().format_shortcut(&COPY));
                if ui.add_enabled(has_doc, copy).clicked() {
                    action = Some(Action::Copy);
                }
                let paste = egui::Button::new("Paste").shortcut_text(ui.ctx().format_shortcut(&PASTE));
                if ui.add(paste).clicked() {
                    action = Some(Action::RequestPaste);
                }
                ui.separator();
                let crop = egui::Button::new("Crop selection").shortcut_text(ui.ctx().format_shortcut(&CROP));
                if ui.add_enabled(self.selection.is_some(), crop).clicked() {
                    action = Some(Action::Crop);
                }
                let resize = egui::Button::new("Resize…").shortcut_text(ui.ctx().format_shortcut(&RESIZE));
                if ui.add_enabled(has_doc, resize).clicked() {
                    action = Some(Action::ShowResize);
                }
            });
            ui.separator();
            if ui.add_enabled(has_doc, egui::Button::new("◀")).on_hover_text("Previous (←)").clicked() {
                action = Some(Action::Step(-1));
            }
            if ui.add_enabled(has_doc, egui::Button::new("▶")).on_hover_text("Next (→)").clicked() {
                action = Some(Action::Step(1));
            }
        });
        action
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(doc) = &self.doc {
                ui.label(format!("{} × {}", doc.image.width(), doc.image.height()));
                if let Some(index) = self.folder.iter().position(|p| Some(p) == doc.path.as_ref()) {
                    ui.separator();
                    ui.label(format!("{}/{}", index + 1, self.folder.len()));
                }
                if let Some(sel) = self.selection {
                    let r = sel.to_pixel_rect();
                    ui.separator();
                    ui.label(format!("Selection {} × {} at {}, {}", r.width, r.height, r.x, r.y));
                }
                ui.separator();
            }
            ui.label(&self.status);
        });
    }

    fn image_view(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let area = response.rect;
        painter.rect_filled(area, 0.0, Color32::from_gray(24));

        let (Some(doc), Some(texture)) = (&self.doc, &self.texture) else { return };
        let image_size = Vec2::new(doc.image.width() as f32, doc.image.height() as f32);
        // Fit to window, but never enlarge small images.
        let scale = (area.width() / image_size.x).min(area.height() / image_size.y).min(1.0);
        let image_rect = Rect::from_center_size(area.center(), image_size * scale);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture.id(), image_rect, uv, Color32::WHITE);

        let to_image = |p: Pos2| ((p - image_rect.min) / scale).to_pos2().clamp(Pos2::ZERO, image_size.to_pos2());
        let to_screen = |p: Pos2| image_rect.min + p.to_vec2() * scale;

        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            let p = to_image(p);
            self.selection = Some(Selection { start: p, end: p });
        }
        if response.dragged()
            && let (Some(p), Some(sel)) = (response.interact_pointer_pos(), &mut self.selection)
        {
            sel.end = to_image(p);
        }
        if response.clicked() {
            self.selection = None;
        }

        if let Some(sel) = self.selection {
            let rect = Rect::from_two_pos(to_screen(sel.start), to_screen(sel.end));
            painter.rect_stroke(rect, 0.0, Stroke::new(3.0, Color32::BLACK), egui::StrokeKind::Middle);
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Middle);
        }
    }

    fn resize_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.resize_dialog else { return };
        let Some(doc) = &self.doc else { return };
        let (orig_w, orig_h) = (doc.image.width(), doc.image.height());
        let mut close = false;
        let mut confirmed = false;

        egui::Window::new("Resize")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Current size: {orig_w} × {orig_h}"));
                egui::Grid::new("resize_grid").show(ui, |ui| {
                    ui.label("Width");
                    if ui.add(egui::DragValue::new(&mut dialog.width).range(1..=65_535)).changed() && dialog.keep_aspect {
                        dialog.height = edit::proportional_height(orig_w, orig_h, dialog.width);
                    }
                    ui.end_row();
                    ui.label("Height");
                    if ui.add(egui::DragValue::new(&mut dialog.height).range(1..=65_535)).changed() && dialog.keep_aspect {
                        dialog.width = edit::proportional_width(orig_w, orig_h, dialog.height);
                    }
                    ui.end_row();
                });
                ui.checkbox(&mut dialog.keep_aspect, "Keep aspect ratio");
                ui.horizontal(|ui| {
                    for percent in [25, 50, 75, 200] {
                        if ui.button(format!("{percent}%")).clicked() {
                            dialog.width = (orig_w * percent / 100).max(1);
                            dialog.height = (orig_h * percent / 100).max(1);
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    confirmed = ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                    close = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });

        if confirmed {
            let (w, h) = (dialog.width, dialog.height);
            self.resize_dialog = None;
            self.apply(|img| edit::resize(img, w, h));
        } else if close {
            self.resize_dialog = None;
        }
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty {
            return;
        }
        self.texture_dirty = false;
        self.texture = self.doc.as_ref().map(|doc| make_texture(ctx, &doc.image));
    }

    fn update_title(&self, ctx: &egui::Context) {
        let title = match &self.doc {
            Some(doc) => {
                let name = doc
                    .path
                    .as_deref()
                    .and_then(Path::file_name)
                    .map_or(Cow::Borrowed("Pasted image"), |n| n.to_string_lossy());
                let star = if doc.modified { " *" } else { "" };
                format!("{name}{star} - img-view")
            }
            None => "img-view".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .first()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
        });
        if let Some(path) = dropped {
            self.open(&path);
        }
        if self.resize_dialog.is_none() && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.selection = None;
        }

        let mut action = self.keyboard_action(ctx);
        egui::Panel::top("menu").show(ui, |ui| {
            action = action.take().or(self.menu_bar(ui));
        });
        if let Some(action) = action {
            self.run(ctx, action);
        }

        self.refresh_texture(ctx);
        self.update_title(ctx);
        self.resize_window(ctx);
        // The resize may have changed the image; upload it before drawing.
        self.refresh_texture(ctx);

        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.image_view(ui));
    }
}

/// Decodes an image, detecting the real format and applying EXIF rotation.
fn load_image(path: &Path) -> Result<DynamicImage> {
    let what = || format!("could not open {}", path.display());
    let mut decoder = ImageReader::open(path)
        .with_context(what)?
        .with_guessed_format()
        .with_context(what)?
        .into_decoder()
        .with_context(what)?;
    let orientation = decoder.orientation().with_context(what)?;
    let mut image = DynamicImage::from_decoder(decoder).with_context(what)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn save_image(image: &DynamicImage, path: &Path) -> Result<()> {
    let is_jpeg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    // JPEG has no alpha channel.
    let result = if is_jpeg {
        image.to_rgb8().save(path)
    } else {
        image.save(path)
    };
    result.with_context(|| format!("could not save {}", path.display()))
}

/// Uploads the image to the GPU, shrinking it first if it exceeds the GPU's texture limit.
fn make_texture(ctx: &egui::Context, image: &DynamicImage) -> egui::TextureHandle {
    let max_side = u32::try_from(ctx.input(|i| i.max_texture_side)).unwrap_or(u32::MAX);
    let shown = if image.width() > max_side || image.height() > max_side {
        Cow::Owned(image.resize(max_side, max_side, FilterType::Triangle))
    } else {
        Cow::Borrowed(image)
    };
    let rgba = shown.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    ctx.load_texture("image", color, egui::TextureOptions::LINEAR)
}
