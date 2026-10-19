# Elocutio for Windows

**Status: planned after the macOS v1.**

The Windows shell will reuse the shared Rust crates and the same provider/voice/analysis pipeline. The platform layer differs:

- **Text access:** UI Automation (UIA) `TextPattern`/`ValuePattern`, with a fallback to the clipboard. Some apps (games, remote desktops, custom-drawn editors) will be unsupported.
- **Write path:** UIA patterns where settable, otherwise clipboard + Ctrl+V with read-back verification.
- **Overlay:** a layered, non-activating window positioned from UIA bounding rectangles; click-through underlines; the card never steals focus.
- **Distribution:** MSIX or a signed NSIS/MSI installer, plus an auto-updater. Code signing with a standard Windows certificate.
- **Browser:** the same Chrome/Edge extension and local bridge as macOS.

Planned structure:

```text
apps/windows/
├── src-tauri/        # Tauri 2 shell, UIA broker, overlay
└── src/              # Svelte UI, shared design tokens
```

Do not start this shell until the macOS M2 overlay and write path are stable. The platform abstractions should be extracted from the macOS spike first, not duplicated.
