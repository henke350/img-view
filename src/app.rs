//! The viewer window: shows one image, browses its folder, crops and resizes.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use egui::{Color32, Key, KeyboardShortcut, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::edit::{self, PixelRect, Turn};
use crate::view::View;
use crate::print::{PrintMode, PrintOptions, Printer};
use crate::{clipboard, folder, print, theme};

const MAX_UNDO: usize = 10;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const CROP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const RESIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
const PRINT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
// Same keys as IrfanView.
const ROTATE_LEFT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::L);
const ROTATE_RIGHT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::R);
// Only shown in tooltips: egui handles these keys itself and sends clipboard events.
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

struct PrintDialog {
    options: PrintOptions,
    /// The default printer's name and page, for the preview. `None` if no printer is installed.
    printer: Option<(String, print::Page)>,
}

enum Action {
    Open,
    Save,
    SaveAs,
    Export,
    Undo,
    Crop,
    ShowResize,
    ShowPrint,
    Step(isize),
    Copy,
    /// Ask egui to read the clipboard; the result arrives as a paste event.
    RequestPaste,
    PasteImage(Arc<egui::ColorImage>),
    PasteText(String),
    Rotate(Turn),
    Zoom(ZoomChange),
}

#[derive(Clone, Copy)]
enum ZoomChange {
    /// Positive zooms in, negative zooms out.
    Steps(f32),
    Actual,
    Fit,
}

pub struct ViewerApp {
    doc: Option<Document>,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    folder: Vec<PathBuf>,
    selection: Option<Selection>,
    resize_dialog: Option<ResizeDialog>,
    print_dialog: Option<PrintDialog>,
    undo: Vec<DynamicImage>,
    status: String,
    view: View,
    /// Where the image was drawn last frame; zoom buttons anchor on its centre.
    last_area: Rect,
    /// False when no Windows icon font was found; buttons then show text labels.
    has_icons: bool,
}

impl ViewerApp {
    pub fn new(start_file: Option<PathBuf>, has_icons: bool) -> Self {
        let mut app = Self {
            doc: None,
            texture: None,
            texture_dirty: false,
            folder: Vec::new(),
            selection: None,
            resize_dialog: None,
            print_dialog: None,
            undo: Vec::new(),
            status: "Press O to open an image, or drop one here.".to_owned(),
            view: View::default(),
            last_area: Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 700.0)),
            has_icons,
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

    /// Writes the image back to its own file; asks for a name if it has none yet.
    fn save(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        let Some(path) = doc.path.clone() else {
            self.save_as();
            return;
        };
        match save_image(&doc.image, &path) {
            Ok(()) => {
                doc.modified = false;
                self.status = format!("Saved {}.", file_name(&path));
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Writes the image to a new file and continues working on that file.
    fn save_as(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        let suggested = doc.path.as_deref().map_or_else(|| "pasted.png".to_owned(), file_name);
        let Some(path) = pick_save_path(doc.path.as_deref(), &suggested) else { return };
        match save_image(&doc.image, &path) {
            Ok(()) => {
                self.folder = folder::images_beside(&path);
                self.status = format!("Saved as {}.", file_name(&path));
                doc.path = Some(path);
                doc.modified = false;
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Writes a copy, usually in another format, and keeps working on the current file.
    fn export(&mut self) {
        let Some(doc) = &self.doc else { return };
        let stem = doc
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| "pasted".to_owned(), |s| s.to_string_lossy().into_owned());
        let Some(path) = pick_save_path(doc.path.as_deref(), &format!("{stem}.png")) else { return };
        self.status = match save_image(&doc.image, &path) {
            Ok(()) => format!("Exported {}.", file_name(&path)),
            Err(e) => format!("{e:#}"),
        };
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

    fn show_print_dialog(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let printer = Printer::default_printer().map(|p| (p.name.clone(), p.page()));
        let options = self.print_dialog.as_ref().map_or_else(PrintOptions::default, |d| d.options);
        self.print_dialog = Some(PrintDialog { options, printer });
    }

    /// Opens the Windows printer dialog and prints. Blocks while the dialog is open.
    fn print(&mut self, options: &PrintOptions) {
        let Some(doc) = &self.doc else { return };
        let Some(printer) = Printer::choose() else {
            self.status = "Printing cancelled.".to_owned();
            return;
        };
        let title = doc.path.as_deref().map_or_else(|| "Pasted image".to_owned(), file_name);
        self.status = match printer.print(&doc.image, options, &title, None) {
            Ok(()) => format!("Sent to {}.", printer.name),
            Err(e) => format!("Could not print: {e:#}"),
        };
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Copy => self.copy(ctx),
            Action::RequestPaste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            Action::PasteImage(image) => self.paste_image(&image),
            Action::PasteText(text) => self.paste_text(&text),
            Action::Open => self.open_dialog(),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Export => self.export(),
            Action::Undo => self.undo(),
            Action::Crop => self.crop_to_selection(),
            Action::ShowResize => self.show_resize_dialog(),
            Action::ShowPrint => self.show_print_dialog(),
            Action::Step(step) => self.step(step),
            Action::Rotate(turn) => self.apply(|img| Ok(edit::rotate(img, turn))),
            Action::Zoom(change) => self.zoom(change),
        }
    }

    fn keyboard_action(&self, ctx: &egui::Context) -> Option<Action> {
        // Don't steal keys while the user is typing into the resize dialog.
        if self.resize_dialog.is_some() || self.print_dialog.is_some() {
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
                // Checked before Save: Ctrl+S would also match Ctrl+Shift+S.
                Some(Action::SaveAs)
            } else if i.consume_shortcut(&SAVE) {
                Some(Action::Save)
            } else if i.consume_shortcut(&EXPORT) {
                Some(Action::Export)
            } else if i.consume_shortcut(&UNDO) {
                Some(Action::Undo)
            } else if i.consume_shortcut(&CROP) {
                Some(Action::Crop)
            } else if i.consume_shortcut(&RESIZE) {
                Some(Action::ShowResize)
            } else if i.consume_shortcut(&PRINT) {
                Some(Action::ShowPrint)
            } else if i.consume_shortcut(&OPEN) {
                Some(Action::Open)
            } else if i.consume_shortcut(&ROTATE_LEFT) {
                Some(Action::Rotate(Turn::Left))
            } else if i.consume_shortcut(&ROTATE_RIGHT) {
                Some(Action::Rotate(Turn::Right))
            } else if i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals) {
                Some(Action::Zoom(ZoomChange::Steps(1.0)))
            } else if i.key_pressed(Key::Minus) {
                Some(Action::Zoom(ZoomChange::Steps(-1.0)))
            } else if i.key_pressed(Key::Num0) {
                Some(Action::Zoom(ZoomChange::Fit))
            } else if i.key_pressed(Key::Num1) {
                Some(Action::Zoom(ZoomChange::Actual))
            } else if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::PageDown) || i.key_pressed(Key::Space) {
                Some(Action::Step(1))
            } else if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::PageUp) || i.key_pressed(Key::Backspace) {
                Some(Action::Step(-1))
            } else {
                None
            }
        })
    }

    /// One row of icon buttons, grouped by job: file, clipboard, edit, zoom, then browsing
    /// on the right. Every button explains itself and its shortcut in a tooltip.
    fn toolbar(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        let has_doc = self.doc.is_some();
        let modified = self.doc.as_ref().is_some_and(|d| d.modified);
        let selected = self.selection.map(Selection::to_pixel_rect).filter(|r| r.width > 0 && r.height > 0);
        // A cloned Context is a cheap handle; borrowing `ui` here would block using it below.
        let ctx = ui.ctx().clone();
        let keys = |s: &KeyboardShortcut| ctx.format_shortcut(s);
        let has_icons = self.has_icons;
        let zoom_percent = self.current_scale().map(|s| format!("{:.0}%", s * 100.0));
        let mut crop_clicked = false;
        let mut actual_size_clicked = false;

        ui.horizontal(|ui| {
            let mut button = |ui: &mut egui::Ui, enabled: bool, icon: Icon, label: &str, tip: String, on_click: Action| {
                let tip = format!("{label}: {tip}");
                if icon_button(ui, has_icons, enabled, icon, label, &tip).clicked() {
                    action = Some(on_click);
                }
            };

            button(ui, true, Icon::OPEN, "Open", "open an image (O)".into(), Action::Open);
            button(ui, modified, Icon::SAVE, "Save", format!("save changes to this file ({})", keys(&SAVE)), Action::Save);
            button(ui, has_doc, Icon::SAVE_AS, "Save as", format!("save to a new file and keep working on it ({})", keys(&SAVE_AS)), Action::SaveAs);
            button(ui, has_doc, Icon::EXPORT, "Export", format!("save a copy in another format ({})", keys(&EXPORT)), Action::Export);
            button(ui, has_doc, Icon::PRINT, "Print", format!("print on paper ({})", keys(&PRINT)), Action::ShowPrint);
            ui.separator();

            let what = if selected.is_some() { "the selection" } else { "the whole image" };
            button(ui, has_doc, Icon::COPY, "Copy", format!("copy {what} ({})", keys(&COPY)), Action::Copy);
            button(ui, true, Icon::PASTE, "Paste", format!("paste an image from the clipboard ({})", keys(&PASTE)), Action::RequestPaste);
            ui.separator();

            // The one accented control: it lights up and shows the exact result once there is a selection.
            let crop_tip = format!("Crop: crop to the selection ({})", keys(&CROP));
            let crop = match selected {
                Some(r) => {
                    let text = format!("{} × {}", r.width, r.height);
                    egui::Button::new(icon_and_text(has_icons, Icon::CROP, &text, theme::ON_MARQUEE))
                        .fill(theme::MARQUEE)
                        .min_size(BUTTON_SIZE)
                }
                None => icon_widget(has_icons, Icon::CROP, "Crop"),
            };
            crop_clicked = ui
                .add_enabled(selected.is_some(), crop)
                .on_hover_text(&crop_tip)
                .on_disabled_hover_text("Crop: drag over the image to select an area first")
                .clicked();
            button(ui, has_doc, Icon::RESIZE, "Resize", format!("change the image size ({})", keys(&RESIZE)), Action::ShowResize);
            button(ui, has_doc, Icon::RotateLeft, "Rotate left", format!("turn a quarter turn anticlockwise ({})", keys(&ROTATE_LEFT)), Action::Rotate(Turn::Left));
            button(ui, has_doc, Icon::RotateRight, "Rotate right", format!("turn a quarter turn clockwise ({})", keys(&ROTATE_RIGHT)), Action::Rotate(Turn::Right));
            button(ui, !self.undo.is_empty(), Icon::UNDO, "Undo", format!("undo the last change ({})", keys(&UNDO)), Action::Undo);
            ui.separator();

            button(ui, has_doc, Icon::ZOOM_OUT, "Zoom out", "make the image smaller (− or scroll down)".into(), Action::Zoom(ZoomChange::Steps(-1.0)));
            // Fixed width so the buttons after it don't shift as the number changes.
            let readout = egui::Button::new(zoom_percent.as_deref().unwrap_or("–")).frame(false).min_size(Vec2::new(52.0, BUTTON_SIZE.y));
            actual_size_clicked = ui
                .add_enabled(has_doc, readout)
                .on_hover_text("Actual size: one image pixel per screen pixel (1). Right-drag to move around a zoomed image.")
                .clicked();
            button(ui, has_doc, Icon::ZOOM_IN, "Zoom in", "make the image larger (+ or scroll up)".into(), Action::Zoom(ZoomChange::Steps(1.0)));
            button(ui, has_doc, Icon::FIT, "Fit", "fit the image to the window (0)".into(), Action::Zoom(ZoomChange::Fit));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_browse = self.folder.len() > 1;
                button(ui, can_browse, Icon::NEXT, "Next", "next image in this folder (→)".into(), Action::Step(1));
                button(ui, can_browse, Icon::PREVIOUS, "Previous", "previous image in this folder (←)".into(), Action::Step(-1));
            });
        });
        if crop_clicked {
            action = Some(Action::Crop);
        }
        if actual_size_clicked {
            action = Some(Action::Zoom(ZoomChange::Actual));
        }
        action
    }

    /// The scale the image was last drawn at, for the zoom readout.
    fn current_scale(&self) -> Option<f32> {
        let doc = self.doc.as_ref()?;
        Some(self.view.scale(image_size(&doc.image), self.last_area.size()))
    }

    /// Zoom from a button or key: anchored on the centre of the image area.
    fn zoom(&mut self, change: ZoomChange) {
        let Some(doc) = &self.doc else { return };
        let (size, area) = (image_size(&doc.image), self.last_area);
        match change {
            ZoomChange::Steps(steps) => self.view.zoom_by(steps, size, area.center(), area.size(), area.center()),
            ZoomChange::Actual => self.view.set_scale(1.0, size, area.center(), area.size(), area.center()),
            ZoomChange::Fit => self.view = View::default(),
        }
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
        painter.rect_filled(area, 0.0, theme::WELL);

        self.last_area = area;

        let (Some(doc), Some(texture)) = (&self.doc, &self.texture) else { return };
        let image_size = image_size(&doc.image);

        // Mouse wheel zooms around the pointer; right or middle drag pans.
        if response.hovered() {
            let scroll: f32 = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                        _ => None,
                    })
                    .sum()
            });
            if scroll != 0.0
                && let Some(pointer) = response.hover_pos()
            {
                self.view.zoom_by(scroll.signum(), image_size, area.center(), area.size(), pointer);
            }
        }
        if response.dragged_by(PointerButton::Secondary) || response.dragged_by(PointerButton::Middle) {
            self.view.pan_by(response.drag_delta(), image_size, area.size());
        }
        // The window may have been resized since the last frame.
        self.view.clamp_pan(image_size, area.size());

        let scale = self.view.scale(image_size, area.size());
        let image_rect = Rect::from_center_size(area.center() + self.view.pan, image_size * scale);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture.id(), image_rect, uv, Color32::WHITE);

        let to_image = |p: Pos2| ((p - image_rect.min) / scale).to_pos2().clamp(Pos2::ZERO, image_size.to_pos2());
        let to_screen = |p: Pos2| image_rect.min + p.to_vec2() * scale;

        if response.drag_started_by(PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            let p = to_image(p);
            self.selection = Some(Selection { start: p, end: p });
        }
        if response.dragged_by(PointerButton::Primary)
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
            painter.rect_stroke(rect, 0.0, Stroke::new(1.5, theme::MARQUEE), egui::StrokeKind::Middle);
        }
    }

    fn print_window(&mut self, ctx: &egui::Context) {
        let (Some(dialog), Some(doc)) = (&mut self.print_dialog, &self.doc) else { return };
        let image_px = (doc.image.width(), doc.image.height());
        let texture = self.texture.as_ref().map(egui::TextureHandle::id);
        let mut print_clicked = false;
        let mut close = false;

        egui::Window::new("Print")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    match &dialog.printer {
                        Some((_, page)) => page_preview(ui, page, image_px, &dialog.options, texture),
                        None => {
                            ui.label("No printer is installed.");
                        }
                    }
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        let o = &mut dialog.options;
                        ui.label("Size on paper");
                        ui.radio_value(&mut o.mode, PrintMode::FitToPage, "Fit to page");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut o.mode, PrintMode::Original, "Original size at");
                            ui.add(egui::DragValue::new(&mut o.image_dpi).range(10.0..=2400.0).suffix(" dpi"));
                        });
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut o.mode, PrintMode::Scale, "Scale to");
                            let scale = ui.add(egui::DragValue::new(&mut o.scale_percent).range(1.0..=1000.0).suffix(" %"));
                            if scale.changed() {
                                o.mode = PrintMode::Scale;
                            }
                            ui.label("of original");
                        });
                        ui.add_space(8.0);
                        if let Some((name, page)) = &dialog.printer {
                            let place = print::layout(image_px, o, page);
                            let (w, h) = place.size_cm(page);
                            ui.label(format!("Prints at {w:.1} × {h:.1} cm, aspect ratio kept."));
                            if !place.fits(page) {
                                ui.colored_label(theme::MARQUEE, "Larger than the page: the edges will be cut off.");
                            }
                            ui.add_space(8.0);
                            ui.weak(format!("Preview for {name}. You can pick another printer, paper and copies next."));
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    print_clicked = ui.add_enabled(dialog.printer.is_some(), egui::Button::new("Print…")).clicked();
                    close = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });

        let options = dialog.options;
        if print_clicked {
            // Keep the options for next time, but close our window before the Windows one opens.
            self.print_dialog = None;
            self.print(&options);
        } else if close {
            self.print_dialog = None;
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
        // A new or changed image (other size, rotated) starts fitted to the window.
        self.view = View::default();
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
        if self.resize_dialog.is_none() && self.print_dialog.is_none() && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.selection = None;
        }

        let mut action = self.keyboard_action(ctx);
        let bar = egui::Frame::new().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(8, 6));
        egui::Panel::top("toolbar").frame(bar).show(ui, |ui| {
            let clicked = self.toolbar(ui);
            action = action.take().or(clicked);
        });
        if let Some(action) = action {
            self.run(ctx, action);
        }

        self.refresh_texture(ctx);
        self.update_title(ctx);
        self.resize_window(ctx);
        self.print_window(ctx);
        // The resize may have changed the image; upload it before drawing.
        self.refresh_texture(ctx);

        let status = egui::Frame::new().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(10, 4));
        egui::Panel::bottom("status").frame(status).show(ui, |ui| self.status_bar(ui));
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

/// A small drawing of the sheet of paper with the image where it will print.
/// The faint outline is the printable area; anything outside it is cut off.
fn page_preview(ui: &mut egui::Ui, page: &print::Page, image_px: (u32, u32), options: &PrintOptions, texture: Option<egui::TextureId>) {
    const MAX_SIDE: f32 = 260.0;
    let paper = Vec2::new(page.paper.0.max(1) as f32, page.paper.1.max(1) as f32);
    let k = (MAX_SIDE / paper.x).min(MAX_SIDE / paper.y);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(MAX_SIDE), Sense::hover());
    let sheet = Rect::from_center_size(rect.center(), paper * k);
    let printable = Rect::from_min_size(
        sheet.min + Vec2::new(page.offset.0 as f32, page.offset.1 as f32) * k,
        Vec2::new(page.printable.0 as f32, page.printable.1 as f32) * k,
    );
    let place = print::layout(image_px, options, page);
    let image_rect = Rect::from_min_size(
        printable.min + Vec2::new(place.x as f32, place.y as f32) * k,
        Vec2::new(place.width as f32, place.height as f32) * k,
    );

    let painter = ui.painter();
    painter.rect_filled(sheet.translate(Vec2::splat(2.0)), 0.0, Color32::from_black_alpha(90));
    painter.rect_filled(sheet, 0.0, Color32::WHITE);
    if let Some(id) = texture {
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.with_clip_rect(printable).image(id, image_rect, uv, Color32::WHITE);
    }
    painter.rect_stroke(printable, 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Outside);
}

fn image_size(image: &DynamicImage) -> Vec2 {
    Vec2::new(image.width() as f32, image.height() as f32)
}

const BUTTON_SIZE: Vec2 = Vec2::new(34.0, 30.0);
const ICON_SIZE: f32 = 16.0;

/// A toolbar icon: a Segoe Fluent Icons glyph, or one we draw where the font has none.
#[derive(Clone, Copy)]
enum Icon {
    Glyph(char),
    RotateLeft,
    RotateRight,
}

impl Icon {
    const OPEN: Self = Self::Glyph('\u{E838}');
    const SAVE: Self = Self::Glyph('\u{E74E}');
    const SAVE_AS: Self = Self::Glyph('\u{E792}');
    const EXPORT: Self = Self::Glyph('\u{EDE1}');
    const PRINT: Self = Self::Glyph('\u{E749}');
    const COPY: Self = Self::Glyph('\u{E8C8}');
    const PASTE: Self = Self::Glyph('\u{E77F}');
    const CROP: Self = Self::Glyph('\u{E7A8}');
    const RESIZE: Self = Self::Glyph('\u{E740}');
    const UNDO: Self = Self::Glyph('\u{E7A7}');
    const ZOOM_OUT: Self = Self::Glyph('\u{E71F}');
    const ZOOM_IN: Self = Self::Glyph('\u{E8A3}');
    const FIT: Self = Self::Glyph('\u{E9A6}');
    const PREVIOUS: Self = Self::Glyph('\u{E76B}');
    const NEXT: Self = Self::Glyph('\u{E76C}');
}

/// The button widget for an icon, before any painting on top of it.
fn icon_widget(has_icons: bool, icon: Icon, label: &str) -> egui::Button<'static> {
    match icon {
        Icon::Glyph(c) if has_icons => {
            egui::Button::new(egui::RichText::new(c.to_string()).font(theme::icon_font(ICON_SIZE))).min_size(BUTTON_SIZE)
        }
        // Drawn icons are painted over an empty button of the same size.
        Icon::RotateLeft | Icon::RotateRight => egui::Button::new("").min_size(BUTTON_SIZE),
        // No icon font on this PC: fall back to words.
        Icon::Glyph(_) => egui::Button::new(label.to_owned()).min_size(Vec2::new(0.0, BUTTON_SIZE.y)),
    }
}

/// A toolbar icon button with a tooltip that explains it, also when it is disabled.
fn icon_button(ui: &mut egui::Ui, has_icons: bool, enabled: bool, icon: Icon, label: &str, tip: &str) -> egui::Response {
    let response = ui
        .add_enabled(enabled, icon_widget(has_icons, icon, label))
        .on_hover_text(tip)
        .on_disabled_hover_text(tip);
    if let Icon::RotateLeft | Icon::RotateRight = icon {
        // Match how egui dims disabled glyphs.
        let color = if enabled { theme::TEXT } else { theme::TEXT.gamma_multiply(0.4) };
        paint_rotate_icon(ui.painter(), response.rect.center(), matches!(icon, Icon::RotateLeft), color);
    }
    response
}

/// Icon followed by text, both in `color`, for the lit-up Crop button.
fn icon_and_text(has_icons: bool, icon: Icon, text: &str, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let format = |font_id| egui::TextFormat { font_id, color, valign: egui::Align::Center, ..Default::default() };
    if let (true, Icon::Glyph(c)) = (has_icons, icon) {
        job.append(&c.to_string(), 0.0, format(theme::icon_font(ICON_SIZE)));
        job.append(text, 8.0, format(egui::FontId::proportional(14.0)));
    } else {
        job.append(&format!("Crop {text}"), 0.0, format(egui::FontId::proportional(14.0)));
    }
    job
}

/// A circular arrow with its gap at the top, like Fluent's refresh glyph (E72C, clockwise),
/// mirrored for anticlockwise. The font has no anticlockwise version, so both are drawn.
fn paint_rotate_icon(painter: &egui::Painter, center: Pos2, anticlockwise: bool, color: Color32) {
    const RADIUS: f32 = 6.0;
    const SEGMENTS: u16 = 24;
    let stroke = Stroke::new(1.4, color);
    let mirror = |p: Pos2| if anticlockwise { Pos2::new(2.0 * center.x - p.x, p.y) } else { p };
    // Angles grow clockwise on screen (y points down). The arc ends near the top.
    let (start, end) = ((-30.0_f32).to_radians(), 255.0_f32.to_radians());
    let arc = (0..=SEGMENTS)
        .map(|i| {
            let t = start + (end - start) * f32::from(i) / f32::from(SEGMENTS);
            mirror(center + Vec2::angled(t) * RADIUS)
        })
        .collect();
    painter.add(Shape::line(arc, stroke));

    // Arrowhead at the end of the arc, pointing along the direction of travel.
    let along = Vec2::new(-end.sin(), end.cos());
    let tip = center + Vec2::angled(end) * RADIUS + along;
    let side = along.rot90() * 3.0;
    let back = tip - along * 3.5;
    painter.add(Shape::line(vec![mirror(back + side), mirror(tip), mirror(back - side)], stroke));
}

const JPEG_QUALITY: u8 = 90;

fn save_image(image: &DynamicImage, path: &Path) -> Result<()> {
    let what = || format!("could not save {}", path.display());
    let is_jpeg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if is_jpeg {
        let file = std::io::BufWriter::new(std::fs::File::create(path).with_context(what)?);
        let encoder = JpegEncoder::new_with_quality(file, JPEG_QUALITY);
        // JPEG has no alpha channel.
        DynamicImage::ImageRgb8(image.to_rgb8())
            .write_with_encoder(encoder)
            .with_context(what)
    } else {
        image.save(path).with_context(what)
    }
}

/// Shows a save dialog in the current image's folder, offering the writable formats.
fn pick_save_path(current: Option<&Path>, suggested_name: &str) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("PNG", &["png"])
        .add_filter("WebP", &["webp"])
        .add_filter("BMP", &["bmp"])
        .add_filter("TIFF", &["tif", "tiff"])
        .set_file_name(suggested_name);
    if let Some(dir) = current.and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_image_should_write_jpeg_from_image_with_alpha() {
        let dir = std::env::temp_dir().join(format!("img-view-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.jpg");
        save_image(&DynamicImage::new_rgba8(8, 6), &path).unwrap();
        let reloaded = load_image(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((reloaded.width(), reloaded.height()), (8, 6));
    }
}
