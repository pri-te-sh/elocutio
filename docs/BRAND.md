# Brand

**Elocutio** · *Your voice. Clearly expressed.*

A private writing companion that refines clarity, correctness, and tone while preserving your voice.

The name comes from the classical rhetorical canon of **elocutio**: the art of choosing words, shaping sentences, and setting tone. The brand should feel like an editor, not a grammar police officer: clear, thoughtful, human.

---

## 1. Logo

The mark is built from two quotation marks and a raised eyebrow:

- **Quotation marks** — language, expression, the writer's words.
- **Raised eyebrow** — attentiveness, friendly guidance, a quiet editorial nod.
- **Asymmetry** — an individual voice; the two marks are deliberately not identical.

| Asset | Use |
| --- | --- |
| `assets/brand/lockup-indigo.png` | Primary lockup on Warm Paper or White |
| `assets/brand/lockup-black.png` | Near-black lockup for high-contrast light surfaces |
| `assets/brand/lockup-white.png` | Reversed lockup on Ink Indigo or dark surfaces |
| `assets/brand/mark-indigo.png` | Standalone mark on light surfaces |
| `assets/brand/mark-black.png` | Standalone mark, monochrome |
| `assets/brand/mark-white.png` | Standalone mark on Ink Indigo |
| `assets/brand/wordmark-white.png` | Reversed wordmark without the mark |

**Rules**

- Keep the logo flat. No gradients, shadows, bevels, outlines, or rotation.
- Do not stretch or condense the mark or wordmark.
- Preserve the eyebrow and the asymmetry; never redraw the marks as a generic quote pair.
- Clear space: at least **2× the eyebrow stroke thickness** on every side.
- Minimum sizes: **24 px** for the standalone mark, **140 px wide** for the lockup.
- Use Ink Indigo on Warm Paper/White, or White on Ink Indigo. Near Black only for monochrome contexts.
- Do not place the logo on busy photography or low-contrast backgrounds.
- Do not use the logo to imply endorsement by Grammarly, Superhuman, Automattic, or Harper.

## 2. Color

| Token | Hex | Use |
| --- | --- | --- |
| **Ink Indigo** | `#191F70` | Primary brand, headlines, links, active UI, app icon field |
| **Near Black** | `#171717` | Body text, monochrome marks, high-contrast UI |
| **Warm Paper** | `#F8F7F3` | Default app background, marketing surfaces |
| **White** | `#FFFFFF` | Cards, reversed logo, clean surfaces |

Indigo leads; neutrals support. Keep large areas neutral and use indigo for emphasis, not for entire reading surfaces.

## 3. Typography

**Inter** is the primary interface and brand typeface.

- **Headings:** Inter SemiBold (600), tight tracking.
- **Body:** Inter Regular (400), 1.5 line height.
- **UI labels:** Inter Medium (500).
- Fallbacks: `-apple-system`, `BlinkMacSystemFont`, `Segoe UI`, `Roboto`, `Helvetica Neue`, Arial, sans-serif.
- Native macOS menu-bar and system dialogs may use SF Pro; this is expected and acceptable.

Type examples:

> **Clear thinking. Natural expression.** — Inter SemiBold
>
> Small edits. Your voice intact. — Inter Regular

## 4. Voice

Three words guide every message:

**Clear** · **Thoughtful** · **Human**

- **Explain the edit.** Say what changed and why; do not just replace.
- **Respect the writer.** Suggestions are offers, not corrections of the person.
- **Stay out of the way.** The best assistant is quiet until it is useful.
- Be specific, short, and calm. Avoid hype, exclamation marks, and moralizing.
- Never claim certainty the product cannot deliver (plagiarism, AI detection, perfect grammar).
- Use "suggestion," "edit," and "explanation," not "error" or "mistake" in user-facing copy where possible.

## 5. Platform icons

The source of truth for icon builds is:

```bash
python3 scripts/build-assets.py
```

| Platform | Asset | Notes |
| --- | --- | --- |
| macOS app | `assets/icons/AppIcon.icns`, `app-icon-1024.png` | Indigo rounded square + white mark |
| macOS menu bar | `assets/icons/menu-bar-template.png` | Monochrome template image, 16–18 px |
| Windows | `assets/icons/windows.ico` | Multi-size ICO, 16–256 px |
| iOS | `assets/icons/ios-1024.png` | Full-bleed indigo, no transparency |
| Android | `assets/icons/android-512.png` | Full-bleed legacy icon; adaptive layers later |
| Chrome/Edge | `assets/icons/browser-{16,32,48,128}.png` | Extension toolbar and store |
| Web | `assets/icons/favicon.ico` | Docs and marketing site |
| Social preview | `assets/icons/social-preview.png` | GitHub repository social preview, 1280×640 |

**App icon rules:** no text, no extra border, preserve the mark's proportions, keep the indigo field flat. The mark occupies roughly two-thirds of the icon's inner field. Do not add a shadow to the artwork; the OS handles presentation.

**Menu bar:** use the monochrome template at 16–18 px. The eyebrow may need to be simplified at that size; preserve the two marks and the raised stroke.

## 6. Usage examples

| Do | Don't |
| --- | --- |
| Indigo lockup on Warm Paper | Indigo lockup on a dark photo |
| White mark on Ink Indigo | Stretched or rotated mark |
| Flat, generous clear space | Gradients, bevels, drop shadows |
| Short, specific explanations | Hype, exclamation marks, moralizing |
| "Suggestion" / "edit" | "Error" / "mistake" in calm UI copy |

## 7. Accessibility

- Ink Indigo `#191F70` on Warm Paper `#F8F7F3` is approximately 11:1 contrast.
- Near Black `#171717` on Warm Paper is approximately 14:1.
- White on Ink Indigo is approximately 12:1.
- Do not rely on color alone for suggestion categories; pair color with labels and icons.
- Respect Reduce Motion; transitions should be 150 ms or less and never block input.

## 8. Files

- Source artwork: [`assets/brand/source/`](../assets/brand/source/)
- Brand guide reference: [`assets/brand/source/brand-guide-v1.jpg`](../assets/brand/source/brand-guide-v1.jpg)
- Design tokens: [`assets/brand/tokens.json`](../assets/brand/tokens.json)
- Generated platform icons: [`assets/icons/`](../assets/icons/)
- Icon build script: [`scripts/build-assets.py`](../scripts/build-assets.py)
