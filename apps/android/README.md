# Elocutio for Android

**Status: planned after the desktop apps.**

Android allows more system-wide integration than iOS, but less than macOS:

- **AccessibilityService:** can read focused text and perform actions in many apps. It requires an explicit, high-friction user permission and is restricted on some OEM builds/Play policies.
- **Custom keyboard (IME):** a reliable fallback and a good surface for inline suggestions; the user switches to the Elocutio keyboard.
- **Overlay:** `TYPE_ACCESSIBILITY_OVERLAY` or a bubble; never steal focus; respect touch/accessibility settings.
- **In-app editor:** full suggestion card, voice profile, and assistant.
- **Voice profiles:** same portable JSON; on-device storage; optional encrypted sync later.
- **On-device model:** a small local model (e.g., via llama.cpp/MLX-equivalent) or BYOK cloud; size and battery constraints apply.
- **Distribution:** Google Play and direct APK for development.

Planned structure:

```text
apps/android/
├── app/                    # Compose app
├── keyboard/               # IME module
├── accessibility/          # optional AccessibilityService module
└── core/                   # JNI/UniFFI bindings to shared Rust crates
```

Start with the keyboard and in-app editor; add the AccessibilityService only after Play policy and OEM testing. Do not claim system-wide support until the app matrix is tested.
