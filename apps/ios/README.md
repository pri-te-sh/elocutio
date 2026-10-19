# Elocutio for iOS

**Status: planned after the desktop apps.**

iOS is a different product shape from macOS:

- **No cross-app text access.** App Sandbox prevents reading or replacing text in other apps. System-wide inline suggestions are not possible.
- **Keyboard extension:** the primary system-wide surface. The user adds the Elocutio keyboard, which can read the text field's context, show suggestions, and insert replacements.
- **Share extension:** "Share to Elocutio" for selected text from any app; returns a rewrite the user can copy or insert.
- **In-app editor:** a focused writing surface with the full suggestion card, voice profile, and assistant.
- **Voice profiles:** the same portable JSON format; samples stay on-device, encrypted; no cloud fine-tuning. Optional iCloud sync is a later, opt-in, end-to-end encrypted feature.
- **On-device model:** Apple Foundation Models where available, with a BYOK cloud fallback for longer text.
- **Distribution:** App Store (keyboard extensions are allowed) and TestFlight; no Developer ID side-loading for most users.

Planned structure:

```text
apps/ios/
├── Elocutio/              # SwiftUI app
├── ElocutioKeyboard/      # keyboard extension
├── ElocutioShare/         # share extension
└── Shared/                # app group storage, fonts, design tokens
```

The shared core will be exposed through a Swift package generated with UniFFI (or a C ABI). Do not port the analysis pipeline to Swift.
