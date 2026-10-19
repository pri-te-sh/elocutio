# Browser extensions

The browser extension is the reliable path for web editors, where native Accessibility is inconsistent. It is also the first cross-platform surface after macOS.

| Browser | Status | Notes |
| --- | --- | --- |
| **Chrome / Edge / Brave / Arc** | M3 | Manifest V3; one codebase |
| **Safari** | Later | Safari Web Extension inside the notarized macOS app |
| **Firefox** | Later | MV3 with a compatibility shim |

## Scope

- Gmail, Notion, Slack web, Linear, and generic `textarea`/`contenteditable`.
- Inline underlines with the CSS Custom Highlight API, plus a closed-shadow-DOM overlay fallback.
- No API keys in the browser. The extension sends text to the local app over a paired loopback WebSocket; the app owns providers and keys.
- **Google Docs is not supported**: canvas rendering has no reliable DOM text. Show a clear unsupported state.
- The extension must work without the app for a read-only demo? No: analysis requires the app. A standalone BYOK mode is out of scope.

See [`extensions/chrome/README.md`](chrome/README.md) and [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) §11.
