# Platform shells

Elocutio ships as **one product with shared core logic and thin platform shells**. macOS is the first target; Windows, the browser extension, iOS, and Android follow.

| Platform | Status | Shell | Text access | Write path |
| --- | --- | --- | --- | --- |
| **macOS** | v1 target | Tauri + Rust + Swift/ObjC for AX | Accessibility API + browser extension | Pasteboard + ⌘V primary; AX fast path |
| **Windows** | After v1 | Tauri + Rust + Win32 | UI Automation (UIA) + browser extension | UIA ValuePattern/TextPattern; clipboard fallback |
| **Chrome/Edge** | M3 | TypeScript MV3 | DOM / contenteditable | DOM APIs + `execCommand`/InputEvent |
| **iOS** | Planned | Native Swift/SwiftUI + keyboard extension | Keyboard extension and share sheet; no cross-app accessibility | Keyboard insertion; in-app editor |
| **Android** | Planned | Kotlin/Compose + IME/accessibility service | AccessibilityService (where permitted) or custom keyboard | Accessibility actions or keyboard insertion |
| **Safari/Firefox** | Later | WebExtension | DOM | DOM APIs |

## Shared crates

These crates must stay platform-agnostic and compile on macOS, Windows, Linux, iOS, and Android:

- `elocutio-core` — domain types and suggestions
- `elocutio-analysis` — chunking, validation, span resolution
- `elocutio-llm` — provider-agnostic requests, responses, errors
- `elocutio-voice` — voice profiles and style cards
- `elocutio-store` — local persistence schema
- `elocutio-bridge` — browser-extension protocol

Platform shells own only:

- focused-field discovery and text capture,
- overlay/window presentation,
- text replacement and undo,
- permissions and distribution,
- platform-specific settings and lifecycle.

## Rules

- Never put provider keys in a platform shell other than the desktop main process.
- Never put platform-specific code into the shared crates.
- Voice profiles are portable JSON; export/import must work across platforms.
- Do not promise cross-app inline suggestions on iOS; the sandbox does not allow it.
- Do not claim Windows/iOS/Android support until the platform has a tested app matrix.
- Keep the platform matrix in this file and in `docs/PLAN.md` current.
