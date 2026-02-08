# SnapCrab 🦀

![SnapCrab Logo](assets/snapcrab.png)

A lightweight, high-performance screenshot and annotation tool for Windows, built with Rust and egui.

## Features

- **Native Capture:** Uses the Windows GDI/DXGI APIs for fast, high-quality screenshots.
- **Interactive Selection:**
  - **Fullscreen:** Capture all monitors instantly.
  - **Window Selection:** Hover over any window on your desktop to highlight and capture it specifically.
  - **Area Selection:** Click and drag to capture a custom rectangular region.
- **Proportional Annotations:**
  - Draw arrows, boxes, and text.
  - Annotations scale with the image resolution, ensuring they look perfect at any zoom level or window size.
- **Layer Management:**
  - Interactive side panel to view, select, and delete individual annotations.
  - Move existing annotations by dragging them directly on the image.
- **Workflow Integration:**
  - **Copy to Clipboard:** Instantly share your annotated screenshot.
  - **Save to File:** Export as PNG with native file dialogs.
- **High DPI Aware:** Sharp UI on 4K and high-resolution displays.
- **Zero Terminal:** Runs as a pure Windows GUI application.

## Building from Source

### Prerequisites

- [Rust](https://rustup.rs/) toolchain.
- For cross-compilation from Linux (NixOS recommended): `cargo-xwin` and `devenv`.

### Build Commands

To build for Windows from Linux using `devenv`:

```bash
cargo xwin build --target x86_64-pc-windows-msvc --release
```

The executable will be located at `target/x86_64-pc-windows-msvc/release/snapcrab.exe`.

## License

MIT / Apache-2.0
