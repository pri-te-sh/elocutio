# Elocutio desktop (macOS)

This directory will hold the forked Harper desktop shell:

```text
apps/desktop/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── tray.rs
│   │   ├── config/
│   │   ├── mac_broker/      # inherited from Harper; audited and rebranded
│   │   ├── highlighter/     # inherited; extended for LLM suggestions
│   │   ├── llm/             # provider orchestration and IPC
│   │   ├── store/
│   │   └── bridge/
│   ├── tauri.conf.json
│   └── Cargo.toml
└── src/                     # Svelte UI
```

## M0 checklist

1. Fetch the pinned upstream source: `./scripts/fetch-upstream.sh`.
2. Copy/adapt `harper-desktop` into this directory; rename product, bundle ID,
   icons, config paths, and updater identity.
3. Add the `elocutio-*` crates to the Tauri workspace.
4. Verify:
   - the overlay appears in TextEdit, Notes, Mail, Safari, and Chrome;
   - the card does not steal focus;
   - a selection rewrite replaces the correct text via pasteboard or the AX
     fast path;
   - an OpenRouter key validates and a structured proofread succeeds.
5. Remove every Harper trademark and URL; retain Apache-2.0 notices.

Do not add app code here until the M0 spike has validated the inherited overlay
and write path. The architecture and decision records explain why.
