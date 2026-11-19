# img-view

A small, fast image viewer for Windows, inspired by IrfanView. A Rust learning experiment.

## Features

- Opens JPEG, PNG, GIF (first frame), BMP, TIFF, WebP, ICO, TGA, QOI, PNM, HDR, EXR
- Respects EXIF rotation from phone and camera photos
- Browse the other images in the same folder
- Crop or copy a dragged selection that you can move and resize afterwards, resize with aspect lock, rotate, undo
- Zoom with the mouse wheel or toolbar, pan with right-drag
- Copy and paste images through the Windows clipboard
- Save, Save as, or Export a copy as JPEG, PNG, WebP, BMP or TIFF
- Print with fit to page, original size (at a chosen dpi) or a scale; aspect ratio kept, live page preview
- Drag and drop a file onto the window
- Toolbar with native Windows 11 icons (Segoe Fluent Icons); hover a button for what it does

## Keys

| Key | Action |
| --- | --- |
| `O` | Open |
| `Ctrl+S` | Save over the current file |
| `Ctrl+Shift+S` | Save as a new file |
| `Ctrl+E` | Export a copy |
| `Ctrl+P` | Print |
| `→` / `Space` / `PgDn` | Next image in folder |
| `←` / `Backspace` / `PgUp` | Previous image |
| left-drag | Select an area (outside it is shaded) |
| drag an edge or corner of the selection | Resize the selection |
| drag inside the selection | Move the selection |
| `Ctrl+Y` / `Enter` | Crop to selection |
| `Ctrl+R` | Resize |
| `L` / `R` | Rotate left / right |
| `Ctrl+C` | Copy selection (or whole image) to clipboard |
| `Ctrl+V` | Paste image from clipboard (or a copied image file path) |
| `Ctrl+Z` | Undo |
| `+` / `-` / mouse wheel | Zoom in / out |
| `0` | Fit to window |
| `1` | Actual size (100%) |
| right-drag or middle-drag | Move around a zoomed image |
| `Esc` | Clear selection |

## Build and run

```
cargo run --release -- path\to\image.jpg
```

The release binary is `target\release\img-view.exe`. To use it from Explorer, right-click an image, choose
**Open with → Choose another app**, and browse to that file.

## Installer

```
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

Builds the release binary and `target\installer\img-view-setup-<version>.exe` with
[Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`). The version comes from
`Cargo.toml`. The installer needs no admin rights (it installs for the current user), adds a Start menu entry, an
optional desktop shortcut and an optional **Open with** entry for image files, and appears in **Settings → Apps**
for uninstalling. It is not code-signed, so Windows SmartScreen may warn the first time it runs.

## Develop

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo test print_to_pdf -- --ignored   # prints a test page to a PDF in %TEMP%
```

Built with [egui/eframe](https://github.com/emilk/egui) for the window, [image](https://github.com/image-rs/image)
for decoding and editing, and [rfd](https://github.com/PolyMeilie/rfd) for file dialogs.
