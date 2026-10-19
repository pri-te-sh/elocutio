# Elocutio browser extension

The Chrome/Edge (Manifest V3) extension lands in M3. It exists because
Accessibility is unreliable for writing in Chromium and because web editors
need precise inline decorations.

Planned structure:

```text
extensions/chrome/
├── manifest.json
├── src/
│   ├── background/      # service worker, bridge connection, pairing
│   ├── content/         # adapters and overlay
│   ├── popup/           # pairing and status
│   └── shared/          # protocol types
└── vite.config.ts
```

## Scope

- Gmail, Notion, Slack web, Linear, and generic `textarea`/`contenteditable`.
- **Google Docs is not supported**: canvas rendering has no reliable DOM text.
- Inline underlines via the CSS Custom Highlight API, with a shadow-DOM
  overlay fallback.
- No API keys in the browser. The extension sends text to the local app over a
  paired loopback WebSocket; the app owns providers and keys.
- One codebase for Chrome/Edge first; Safari is post-1.0.

See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §11 for the bridge and
adapter design.
