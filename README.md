# iPaste

> A local-first, keyboard-friendly desktop clipboard manager that turns temporary copies into searchable, organized, reusable workflow pieces.

**Languages:** English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md)

iPaste lives in your system tray and records clipboard history locally. Open the panel with a global shortcut, search previous content, press Enter to paste, or save frequently used snippets into categories for long-term reuse.

It is built for people who move between chat, browsers, terminals, design tools, notes, and code editors all day. Links, commands, color values, prompts, reply templates, and screenshot text do not need to disappear into temporary files or old chat threads.

![iPaste desktop preview](docs/assets/ipaste-app-preview.jpg)

## Features

- Local first: clipboard history is stored in a local SQLite database on the current device.
- Fast access: open the panel with <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd>, or customize the shortcut in settings.
- Multiple content types: text, links, colors, HTML snippets, images, and file clipboard entries.
- Search and keyboard flow: optimized for quick lookup, selection, and Enter-to-paste.
- Saved categories: keep reusable snippets for code, commands, addresses, reply templates, prompts, and more.
- Image viewer: preview, zoom, rotate, copy back to the clipboard, and extract text with OCR.
- Append copy: temporarily merge several text copies into one snippet while gathering material.
- Cross-device sync: pair two devices across the internet by exchanging a one-time invite ticket — clipboard content travels directly between them with end-to-end encryption (QUIC + NAT hole punching, relayed as ciphertext when punching fails); no cloud account needed, with multi-device management, revocation, and automatic reconnection.
- Quick actions: save shell commands as one-keystroke panel actions, with optional confirmation, streamed output, and JSON import/export.
- Configurable preferences: retention period, panel layout, default open behavior, global shortcut, language, and OCR mode.
- Optional self-hosted sync: sync only saved categories and saved text-like content; raw clipboard history stays local.
- Signed updates: built-in Tauri updater support for releases distributed through GitHub Releases or Cloudflare R2.

## Download

Download the latest build from [Releases](https://github.com/iPaste-app/iPaste/releases/latest).

Current release targets:

| Platform | Architecture | Notes |
| --- | --- | --- |
| Windows | x64 | Uses the system WebView2 Runtime; install it first if it is missing. |
| macOS | Apple Silicon | Auto paste requires Accessibility permission. |
| macOS | Intel | Auto paste requires Accessibility permission. |

Linux is not an official target yet. Tauri is cross-platform, but this repository currently focuses on macOS and Windows validation.

### macOS: "iPaste is damaged and can't be opened"

The macOS installers are not notarized by Apple yet, so Gatekeeper may block the app when it is first downloaded from a browser, showing "iPaste is damaged and can't be opened. You should move it to the Trash."

The app itself is fine. Install it with the one-click script, which copies the app to /Applications and removes the quarantine flag:

```bash
bash <(curl -fsSL https://github.com/huangsheng6668/iPaste/releases/latest/download/install-macos.sh)
```

Or remove the flag manually after dragging the app to /Applications:

```bash
xattr -dr com.apple.quarantine /Applications/iPaste.app
```

If you already have iPaste installed, use **Check for Updates** inside the app — update packages are not affected by Gatekeeper.

### macOS permissions

iPaste needs two separate permissions on macOS. Grant them individually under **System Settings → Privacy & Security**:

- **Accessibility**: for auto paste (simulating keystrokes).
- **Screen Recording**: for screenshot OCR. This is a separate permission from Accessibility — screenshot OCR won't work with only Accessibility enabled.

After toggling a permission, fully quit and restart iPaste for it to take effect. Because the installers are unsigned, **permissions may stop working after each iPaste update** (the switch still appears on in System Settings but the app reports "not granted"): remove iPaste from the permission list, add it again, toggle it on, and restart the app.

## Quick Start

1. Launch iPaste. It stays in the tray and starts listening to the clipboard.
2. Copy text, links, colors, or images as usual.
3. Press <kbd>Command</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> or <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> to open the panel.
4. Search, select an item, and press Enter to paste it back into the active app.
5. Save long-term reusable content into categories and organize it around your workflow.

Auto paste on macOS requires Accessibility permission. Image OCR on Windows requires downloading PaddleOCR models from Settings.

## Privacy And Data

iPaste is local-first by default.

- Automatically captured clipboard history is not uploaded or synced.
- Local data is stored in a SQLite database under the system app data directory.
- Cross-device sync transfers content directly between your own devices across the internet. Trust is established with a one-time invite ticket; transport is end-to-end encrypted via QUIC TLS (NAT hole punching, with a relay forwarding only ciphertext when punching fails); no cloud account is involved.
- When cloud sync is enabled, only categories and saved text, link, color, and HTML entries are synced.
- Image and file snippets are currently excluded from the cloud sync payload.
- Cloud sync requires your own API address and API key.
- The updater verifies signed release artifacts before installation.

If your clipboard often contains passwords, keys, client data, or internal company content, confirm your team security rules before using any clipboard manager.

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | OCR uses the system Vision framework; auto paste requires Accessibility permission. |
| Windows | Supported | OCR uses downloadable PaddleOCR models. |
| Linux | Not supported yet | No official release or full validation at the moment. |

## Tech Stack

- Tauri 2: desktop shell, tray, windows, updater, and system integration.
- Rust: clipboard capture, SQLite storage, global shortcuts, paste automation, OCR pipeline, and sync orchestration.
- Vue 3, TypeScript, Pinia, Vite, Tailwind CSS 4: app UI.
- `rusqlite`: local SQLite persistence.
- Cloudflare Pages/Workers-compatible API: optional sync service.

## Development

### Requirements

- Node.js 22 or newer.
- npm 10 or newer.
- Rust stable toolchain.
- Tauri 2 platform dependencies for your operating system.

macOS development requires Xcode Command Line Tools. Windows development requires Microsoft C++ Build Tools; install WebView2 Runtime too if it is missing.

### Install Dependencies

```bash
npm install
```

### Web Preview

```bash
npm run dev
```

The browser preview uses mock data when native Tauri APIs are unavailable. It is useful for UI work, but it does not capture the real system clipboard.

### Desktop Development

```bash
npm run tauri dev
```

### Build

```bash
npm run lint        # ESLint
npm test            # Vitest unit tests (frontend)
npm run build       # Type-check (vue-tsc) + Vite production build
npm run tauri build # Desktop installers
```

Quick native compile check:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

### Shared Types

TypeScript bindings in `src/types/generated/` are generated from Rust via ts-rs. After changing shared models in `models.rs` or event payloads/names in `events.rs`, regenerate and commit them — CI verifies freshness:

```bash
npm run gen:types
```

## Project Structure

```text
.
├── src/                  # Vue app: components, composables, Pinia stores, frontend API wrappers
├── src-tauri/            # Tauri config and Rust desktop backend
│   └── src/              # Rust backend modules (see below)
├── scripts/              # Release, versioning, and updater distribution tools
├── docs/                 # Operational docs and project notes
├── key/                  # Public updater key; private keys must not be committed
└── .github/workflows/    # CI and signed desktop build release workflows
```

The Rust backend in `src-tauri/src/` is split into small domain modules:

| Module | Responsibility |
| --- | --- |
| `lib.rs` | Tauri builder entry (`run()` composition root) and shared constants |
| `models.rs` | Structured serde data models shared by commands and modules (exported to TypeScript via ts-rs) |
| `error.rs` | `AppError`: unified command error contract (`{code, message, params}`) |
| `events.rs` | Single source of frontend/backend event names and payloads; generates `src/types/generated/events.ts` |
| `util.rs` | Shared pure helpers: hashing, clip-type detection, `clean_*` validation, localized labels |
| `store.rs` + `store/` | SQLite persistence split by domain (clips/categories/settings/automations/sync/migrations/secrets) |
| `clipboard.rs` | Clipboard capture, normalization, and write-back |
| `cloud.rs` | Self-hosted sync API client |
| `lan_sync/` | Cross-device sync (v5): iroh QUIC transport, one-time invite tickets, device identity and trust store, multi-device link registry, pairing guard |
| `ocr/` | Image OCR: asset installer and status (Windows), PaddleOCR runner (Windows), Vision pipeline (macOS) |
| `window.rs` | Panel/settings/viewer windows, native panel behavior, window positioning |
| `tray.rs` | System tray, menu labels, menu event handling |
| `shortcut.rs` | Global shortcut registration and updates |
| `paste.rs` | Target app activation and paste triggering |
| `automation.rs` | Quick-action process execution and event streaming |
| `commands.rs` | Thin Tauri command layer exposing domain modules to the UI |

## How It Works

### Clipboard Capture

The Rust backend listens to the system clipboard, normalizes supported content, writes it to SQLite, and emits updates to the Vue panel. Text-like snippets are deduplicated by content hash. Image snippets are stored as local app data resources and rendered through the Tauri resource protocol.

### Applying Snippets

When pasting from iPaste, the app writes the selected snippet back to the system clipboard, then triggers the platform paste shortcut. Direct paste on macOS requires Accessibility permission.

### Saved Categories

History items and saved category items are different concepts. History items expire according to the retention policy. Saved category items are explicit snapshots kept until you delete them.

### Cloud Sync

The desktop app can connect to a self-hosted iPaste sync API using an API address and API key in Preferences. Sync scope includes categories and saved text-like category items. The sync service source will be open-sourced when it is ready.

### Cross-Device Sync

Two iPaste instances pair by exchanging a one-time invite ticket: one device creates the invite, the other joins with the ticket, and both sides confirm before any transfer. Devices connect directly across the internet over QUIC (NAT hole punching; a relay forwards only ciphertext when punching fails), and clips and whole categories flow between paired devices — a category that does not exist on the receiving side is created automatically. Paired devices can be managed or revoked anytime, and connections reconnect automatically after drops.

### Quick Actions

Quick actions are saved shell commands shown in their own panel category. Run them with one keystroke, optionally confirm first, watch streamed output in the detail pane, and share sets between machines via JSON import/export.

### Image OCR

macOS uses the system Vision framework. Windows uses PaddleOCR models that can be installed from app preferences.

## Contributing

Issues, ideas, and pull requests are welcome.

Before submitting a pull request, run at least:

```bash
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

Building the Rust backend on Windows needs libclang for bindgen (used by the PaddleOCR engine). Install it with `choco install llvm`, or use `pip install libclang` and point `LIBCLANG_PATH` at the `clang/native` folder inside your Python site-packages.

If your change touches shared Rust models or events, also run `npm run gen:types` and commit the regenerated bindings.

Please keep the project local-first, privacy-conscious, and careful around any change that syncs user data. For larger features, open an issue first to discuss boundaries and interaction design.

## License

This project is licensed under Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

When redistributing, keep the license, copyright, and NOTICE information; modified files must document their changes.
