# img-view

A small, fast image viewer for Windows, inspired by IrfanView. A Rust learning experiment.

## Features

- Opens JPEG, PNG, GIF (first frame), BMP, TIFF, WebP, ICO, TGA, QOI, PNM, HDR, EXR
- Respects EXIF rotation from phone and camera photos
- Browse the other images in the same folder
- Crop to a dragged selection, resize with aspect lock, undo
- Copy and paste images through the Windows clipboard
- Save as JPEG, PNG, WebP, BMP or TIFF
- Drag and drop a file onto the window

## Keys

| Key | Action |
| --- | --- |
| `O` | Open |
| `Ctrl+S` | Save as |
| `→` / `Space` / `PgDn` | Next image in folder |
| `←` / `Backspace` / `PgUp` | Previous image |
| drag with mouse | Select an area |
| `Ctrl+Y` | Crop to selection |
| `Ctrl+R` | Resize |
| `Ctrl+C` | Copy selection (or whole image) to clipboard |
| `Ctrl+V` | Paste image from clipboard (or a copied image file path) |
| `Ctrl+Z` | Undo |
| `Esc` | Clear selection |

## Build and run

```
cargo run --release -- path\to\image.jpg
```

The release binary is `target\release\img-view.exe`. To use it from Explorer, right-click an image, choose
**Open with → Choose another app**, and browse to that file.

## Develop

```
cargo test
cargo clippy --all-targets -- -D warnings
```

Built with [egui/eframe](https://github.com/emilk/egui) for the window, [image](https://github.com/image-rs/image)
for decoding and editing, and [rfd](https://github.com/PolyMeilie/rfd) for file dialogs.
