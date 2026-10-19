# Vendored Harper subset

This directory contains the minimal Harper source needed by the macOS shell:

| Crate | Why |
| --- | --- |
| `harper-core` | Local English grammar engine |
| `harper-brill` | Part-of-speech tagging used by `harper-core` |
| `harper-pos-utils` | POS helpers used by `harper-brill` |
| `harper-dictionary-wordlist` | Dictionary loading/saving for user words |

**Upstream:** [Automattic/harper](https://github.com/Automattic/harper) v2.13.0,
commit `f4ebc61d77df1fc00b6aa8bc604e91af3534dad1` (see `../../upstream.lock`).
**License:** Apache-2.0; see `LICENSE` and `../../NOTICE`.

## Deliberate patches

1. **`harper-core` thesaurus disabled.** The optional `harper-thesaurus` crate is
   ~24 MB of data and is unused by the desktop shell. The optional dependency
   was removed, `default` is now `["concurrent"]`, and a no-op `thesaurus`
   feature stub remains so existing `cfg(feature = "thesaurus")` code compiles
   without warnings. This does not change grammar checking.
2. No source edits to the linting logic. Elocutio-specific behavior lives in
   `apps/macos`, not here.

If you need the thesaurus later, restore the dependency from upstream and add
the crate under this directory rather than editing `harper-core` ad hoc.

## Updating

Do not run a blind copy over this directory. Follow `../../UPSTREAM.md`:

1. Fetch the desired Harper tag into `.upstream/harper`.
2. Diff the four crates against this directory.
3. Copy only the upstream changes, re-apply the thesaurus patch, and record the
   new commit in `upstream.lock`.
4. Rebuild `apps/macos` and run the overlay matrix in
   `docs/M0-VERIFICATION.md`.

The desktop shell is **not** vendored from `harper-desktop`; its source was
copied into `apps/macos` and is maintained as Elocutio code under Apache-2.0.
