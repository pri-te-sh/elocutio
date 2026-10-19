# Building a Grammarly-like background writing assistant for macOS

**Research date:** 2026-10-19
**Target:** macOS 14 / 15 / 26 (Tahoe), Apple Silicon, Swift 6.3, Xcode 26
**Verified against:** macOS 26.6.2 (build 25G83), Swift 6.3.3, SDK `MacOSX26.sdk` at `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`, plus live AX experiments on that machine.

> **Method note.** The session's `web_search` endpoint was misconfigured (HTTP 404) for the entire research window, so this report was built from: (a) the **macOS 26 SDK headers on this machine** — the most authoritative source for what the API surface actually is in 2026; (b) Apple's documentation JSON API (`developer.apple.com/tutorials/data/...`); (c) **Chromium source** (`ui/accessibility/platform/browser_accessibility_cocoa.mm`) for how Chrome/Electron actually answer AX queries; (d) vendor blogs (Google Workspace, Sparkle) and open-source READMEs; (e) live AX experiments. Claims that could not be verified this way are explicitly marked **[unverified]**. There is a verification log in Appendix A.

---

## 0. Executive summary

A Grammarly-class assistant on macOS is **three products stapled together**, and only one of them is hard:

1. **A read path** — Accessibility (`AXUIElement`) for native/web text, with OCR as a fallback. Works today, is well documented, but is *read-mostly* in Chromium-based apps.
2. **A write path** — there is **no reliable, universal "replace text" API**. AX `AXValue` setting works in AppKit apps and is a **no-op in Chromium/Electron**; `CGEventKeyboardSetUnicodeString` is explicitly documented as ignorable by app frameworks. The only near-universal mechanism is **pasteboard + synthesized ⌘V**, with AX-set as a fast path and a verification/read-back step.
3. **A focus-preserving overlay** — a `NSPanel` with `.nonactivatingPanel` that never becomes key, positioned by converting AX screen rects into Cocoa coordinates.

The unavoidable product constraints:

* **App Sandbox cannot do this.** Reading other apps' text via AX requires a non-sandboxed, **Developer ID-signed + notarized** app outside the Mac App Store. Your Safari extension therefore must also ship in the Developer ID app, not as an MAS-only product.
* **Permissions are the product.** Accessibility is mandatory; Input Monitoring is needed only if you tap keys; Screen Recording only if you fall back to OCR. TCC identity is tied to code signature, so **signing stability is a hard engineering requirement**, not a release chore.
* **Google Docs is the single biggest hole** — canvas rendering since 2021 removed the DOM, so neither AX nor a content script sees the text reliably.

**Recommended MVP:** menu-bar agent (LSUIElement) + AX read (focused element, selected range, `AXStringForRange`) + pasteboard/⌘V write with clipboard restore + non-activating `NSPanel` card + Chrome/Edge MV3 extension using content scripts. Defer Safari, Docs, and inline underlines.

---

## 1. Accessibility API (AXUIElement) text access

### 1.1 The core call chain

Everything starts from the system-wide element and a single attribute walk. The functions are declared in `HIServices/AXUIElement.h` and are all present and non-deprecated in the macOS 26 SDK:

```swift
import ApplicationServices

// 1. Global element (no app targeting needed)
let systemWide = AXUIElementCreateSystemWide()

// 2. Focused element, regardless of which app is frontmost
var focusedRef: CFTypeRef?
let err = AXUIElementCopyAttributeValue(
    systemWide,
    kAXFocusedUIElementAttribute as CFString,
    &focusedRef
)
guard err == .success, let focused = focusedRef as! AXUIElement? else { return }
```

`AXUIElementCreateSystemWide` is documented in the header as existing precisely for this: *"This is useful for things like finding the focused accessibility object regardless of which application is currently active."*

From the focused element, the text-relevant attributes are:

| Attribute | Type | Writable? | Notes |
|---|---|---|---|
| `kAXRoleAttribute` (`AXRole`) | CFString | no | `AXTextField`, `AXTextArea`, `AXComboBox`, `AXWebArea`, … |
| `kAXSubroleAttribute` | CFString | no | e.g. `AXSecureTextField` — **bail out immediately** |
| `kAXValueAttribute` (`AXValue`) | CFString / NSAttributedString | app-dependent | Full field contents. Settable in most AppKit text views; **ignored by Chromium** (§1.5) |
| `kAXSelectedTextAttribute` | CFString | **no** | Header: *"Writable? No."* Do not rely on setting it |
| `kAXSelectedTextRangeAttribute` | AXValue(`CFRange`) | **yes** | Header: *"Writable? Yes."* Setting it moves the caret/selection |
| `kAXSelectedTextRangesAttribute` | CFArray of CFRange | yes | Non-contiguous selections (rare) |
| `kAXNumberOfCharactersAttribute` | CFNumber | no | Cheap length check before pulling megabytes |
| `kAXVisibleCharacterRangeAttribute` | AXValue(`CFRange`) | no | Required for `AXTextArea`; useful for viewport-limited work |
| `kAXFrameAttribute` | AXValue(`CGRect`) | no | Element rect in **global top-left** screen coords |
| `kAXFocusedAttribute` | CFBoolean | yes | Can re-focus the element after injection |

The header is unusually explicit about which of these are settable, and that matters: a lot of blog code does `AXUIElementSetAttributeValue(el, kAXSelectedTextAttribute, newText)`, which is **wrong per the API contract** and silently fails in most apps.

```swift
// Read the whole field (guard on size first)
func value(of el: AXUIElement) -> String? {
    var n: CFTypeRef?
    guard AXUIElementCopyAttributeValue(el, kAXValueAttribute as CFString, &n) == .success
    else { return nil }
    return n as? String
}

// Read just the selection
func selectedText(of el: AXUIElement) -> String? {
    var s: CFTypeRef?
    guard AXUIElementCopyAttributeValue(el, kAXSelectedTextAttribute as CFString, &s) == .success
    else { return nil }
    return s as? String
}

// Read/set the selection range
func selection(of el: AXUIElement) -> CFRange? {
    var v: CFTypeRef?
    guard AXUIElementCopyAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, &v) == .success,
          let axv = v as! AXValue? else { return nil }
    var r = CFRange()
    guard AXValueGetValue(axv, .cfRange, &r) else { return nil }
    return r
}

func setSelection(_ r: CFRange, on el: AXUIElement) {
    var r = r
    if let v = AXValueCreate(.cfRange, &r) {
        AXUIElementSetAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, v)
    }
}
```

### 1.2 Parameterized attributes — the workhorses

These take an argument and are how you avoid slurping an entire 200-page document:

| Parameterized attribute | Parameter | Returns | Use |
|---|---|---|---|
| `AXStringForRange` | AXValue(`CFRange`) | CFString | Read a paragraph/window of text |
| `AXRangeForLine` | CFNumber | AXValue(`CFRange`) | Map a paragraph index to a range |
| `AXLineForIndex` | CFNumber | CFNumber | Inverse mapping |
| `AXBoundsForRange` | AXValue(`CFRange`) | AXValue(`CGRect`) | **Screen rect for a word/sentence — needed for inline underlines** |
| `AXRangeForPosition` | AXValue(`CGPoint`) | AXValue(`CFRange`) | Hit-test a click to a character |
| `AXAttributedStringForRange` | AXValue(`CFRange`) | NSAttributedString | Rich text: bold/italic runs |
| `AXRTFForRange` | AXValue(`CFRange`) | NSData (RTF) | Legacy rich text |
| `AXStyleRangeForIndex` | CFNumber | AXValue(`CFRange`) | Run boundaries |
| `AXUIElementsForSearchPredicate` | NSDictionary | NSArray | **New in macOS 26** — server-side element search instead of recursive walks |

```swift
func string(in range: CFRange, of el: AXUIElement) -> String? {
    var r = range
    guard let p = AXValueCreate(.cfRange, &r) else { return nil }
    var out: CFTypeRef?
    let e = AXUIElementCopyParameterizedAttributeValue(
        el, kAXStringForRangeParameterizedAttribute as CFString, p, &out)
    return e == .success ? out as? String : nil
}

func screenRect(for range: CFRange, of el: AXUIElement) -> CGRect? {
    var r = range
    guard let p = AXValueCreate(.cfRange, &r) else { return nil }
    var out: CFTypeRef?
    guard AXUIElementCopyParameterizedAttributeValue(
        el, kAXBoundsForRangeParameterizedAttribute as CFString, p, &out) == .success,
        let axv = out as! AXValue? else { return nil }
    var rect = CGRect.zero
    return AXValueGetValue(axv, .cgRect, &rect) ? rect : nil
}
```

The header itself warns that these are **optional per element** — always probe with `AXUIElementCopyParameterizedAttributeNames` before assuming `AXBoundsForRange` exists.

### 1.3 Text markers and attributed strings (10.15+/12+)

`AXTextMarkerRef` / `AXTextMarkerRangeRef` are the "modern" text addressing scheme, declared in the same header (`AXTextMarkerCreate`, `AXTextMarkerRangeCreate`, `AXTextMarkerRangeCopyStartMarker`, …). They matter because:

* `kAXSelectedTextMarkerRangeAttribute` is what **Chromium actually implements as settable** (see §1.5), so this is your reliable way to move the selection inside Chrome/Electron.
* `AXAttributedStringForRange` returns runs, and **macOS 26 added the attributes you need to reason about them**: `NSAccessibilityFontBoldAttribute` and `NSAccessibilityFontItalicAttribute` are now `NSAttributedStringKey`s available in **macOS 26.0**:

```objc
// NSAccessibilityConstants.h (macOS 26 SDK)
APPKIT_EXTERN NSAttributedStringKey const NSAccessibilityFontBoldAttribute   API_AVAILABLE(macos(26.0));
APPKIT_EXTERN NSAttributedStringKey const NSAccessibilityFontItalicAttribute API_AVAILABLE(macos(26.0));
APPKIT_EXTERN NSAccessibilityAttributeName const NSAccessibilityBlockQuoteLevelAttribute API_AVAILABLE(macos(26.0));
```

This is genuinely useful for a writing assistant: you can skip suggestions inside code blocks/bold headings and you can style your underlay to match. Before 26 you had to parse RTF.

### 1.4 App-family support matrix

What you can expect when you ask a focused element for `AXValue` / `AXSelectedText` / `AXSelectedTextRange`:

| Family | Examples | Read value | Read selection | Set selection | Set value | Notes |
|---|---|---|---|---|---|---|
| **AppKit** (`NSTextView`, `NSTextField`) | Notes, Mail, Xcode editor, TextEdit | ✅ | ✅ | ✅ | ✅ (usually) | Best case. `AXBoundsForRange` works for underline placement |
| **SwiftUI** (`TextField`, `TextEditor`) | small SwiftUI apps | ✅ | ⚠️ mostly | ⚠️ | ⚠️ | SwiftUI on macOS is AppKit-backed, but `TextEditor` bridging has historically been quirky. Test per app |
| **Chromium / Electron** | Chrome, Edge, Arc, Slack, VS Code, Notion desktop, Discord | ✅ | ✅ | ✅ via text-marker range | ❌ | Full detail below |
| **Safari / WebKit** | Safari, Mail web views | ✅ | ✅ | ✅ | ⚠️ sometimes | WebKit's AX is generally better-behaved than Chromium's |
| **Java (Swing/AWT)** | IDE installers, legacy enterprise | ❌ mostly | ❌ | ❌ | ❌ | Java Access Bridge on macOS is weak; plan on OCR |
| **Qt** | Telegram desktop (older), VLC, some DAWs | ⚠️ | ⚠️ | ⚠️ | ⚠️ | Qt implements NSAccessibility for many widgets but text attributes are inconsistent **[unverified]** |
| **Terminal emulators** | Terminal, iTerm2 | ⚠️ | ⚠️ | ❌ | ❌ | Terminal surfaces scrollback to AT but selection semantics are unusual; **do not inject here** |
| **Secure fields** | password fields | ❌ | ❌ | ❌ | ❌ | `AXSubrole == AXSecureTextField`; see secure input in §3 |

**Chromium is the critical case** because so much writing happens there. The actual implementation in `browser_accessibility_cocoa.mm`:

```objc
// Chromium: which attributes accept a set
- (BOOL)accessibilityIsAttributeSettable:(NSString*)attribute {
  if ([attribute isEqualToString:NSAccessibilityFocusedAttribute]) { ... }
  if ([attribute isEqualToString:NSAccessibilityValueAttribute]) {
    return _owner->HasAction(ax::mojom::Action::kSetValue);
  }
  if ([attribute isEqualToString:NSAccessibilitySelectedTextRangeAttribute] &&
      _owner->HasState(ax::mojom::State::kEditable)) { return YES; }
  if ([attribute isEqualToString:CFToNSPtrCast(kAXSelectedTextMarkerRangeAttribute)]) { return YES; }
  return NO;
}

- (void)accessibilitySetValue:(id)value forAttribute:(NSString*)attribute {
  if ([attribute isEqualToString:NSAccessibilityFocusedAttribute]) { ... }
  if ([attribute isEqualToString:NSAccessibilitySelectedTextRangeAttribute]) { ... setAccessibilitySelectedTextRange ... }
  if ([attribute isEqualToString:CFToNSPtrCast(kAXSelectedTextMarkerRangeAttribute)] && editable) {
      manager->SetSelection(...);   // DOM selection only — no text mutation
  }
}
```

Read that setter carefully: it handles **focus, selection range, and text-marker range — and nothing else**. There is no branch that sets `AXValue`, and no branch that sets `AXSelectedText`. So in Chrome, Edge, Arc, Electron apps:

* **Reading works well.** `AXValue`, `AXSelectedText`, `AXStringForRange`, `AXRangeForLine`, `AXBoundsForRange`, `AXAttributedStringForRange` are all implemented (`accessibilityStringForRange:`, `accessibilityRangeForLine:`, `NSAccessibilityBoundsForRangeParameterizedAttribute` are in the same file).
* **Writing does not.** You can *select* a range, but the only way to replace it is to send keyboard input or paste.

One practical trick for Chromium: the a11y tree is built lazily. Tools that drive Chrome often set the private attribute `AXEnhancedUserInterface` / call `AXUIElementSetAttributeValue(app, "AXManualAccessibility", kCFBooleanTrue)` to force the renderer's accessibility mode on. These are **private, undocumented** and can change; treat as an optimization, not a dependency. **[unverified for macOS 26]** — verify empirically against Chrome 14x before shipping.

### 1.5 Web areas and Google Docs

A web page inside Chrome/Safari is an `AXWebArea` (role constant in `AXWebConstants.h`) with a DOM-derived subtree. Practical implications:

* Content inside `<textarea>` / `<input>` / `contenteditable` is reachable as normal `AXTextField` / `AXTextArea` nodes **if** the browser exposes them — Chromium does.
* `AXWebArea` itself usually has **no** `AXValue`. Do not read it. Walk to the focused descendant instead.
* Getting "the paragraph the caret is in" from a web area means `AXRangeForLine(AXLineForIndex(selectedRange.location))` and then `AXStringForRange`. This works in Chromium but line indices are **visual/logical lines in the render tree**, not `\n`-separated paragraphs. For `contenteditable`, a "line" is often a block element. Budget time here.

**Google Docs** is the outlier and deserves its own paragraph. Google moved Docs to **canvas-based rendering** starting May 2021, explicitly warning extension authors:

> "we'll be migrating the underlying technical implementation of Docs from the current HTML-based rendering approach to a canvas-based approach… this may impact some Chrome extensions, where they may no longer work as intended… Some Chrome extensions rely on the way the backend of a Google Doc is structured or specific bits of HTML to function properly."
> — [Google Workspace Updates, 2021-05-11](https://workspaceupdates.googleblog.com/2021/05/Google-Docs-Canvas-Based-Rendering-Update.html)

Google states assistive technologies are *not* impacted — i.e. Docs maintains a parallel accessibility tree. In practice that means AX can sometimes read Docs text (the canvas has an AT-visible representation) but **the DOM has no text and no ranges to decorate**. Consequences:

* A content script **cannot** read or underline Docs text by DOM inspection.
* Building a Docs integration means either (a) the **Google Docs Add-on** framework (Apps Script, sidebar UI, `DocumentApp` API — server-side, no inline decorations over the canvas), (b) the **Google Docs API** with OAuth (read/write whole documents, high latency, not inline), or (c) OCR over the canvas region (needs Screen Recording; loses caret mapping).
* **Recommendation: explicitly declare Docs unsupported in v1** and say so in onboarding. This is what most small tools do.

### 1.6 Threading, timeouts, error codes

AX calls are **synchronous Mach IPC to another process**. They block the calling thread until the target app replies. Rules:

1. **Never call AX on the main thread.** A hung target app (beach-balling Word, a paused Electron renderer) will freeze your UI for the timeout duration.
2. **Set a timeout, globally, at launch.** The header:

   > "Pass the system-wide accessibility object if you want to set the timeout globally for this process… Setting `timeoutInSeconds` to 0 for the system-wide accessibility object resets the global timeout to its default value. Setting `timeoutInSeconds` to 0 for any other accessibility object makes that element use the current global timeout value."

   ```swift
   AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 0.25) // global, 250 ms
   ```

   The default is generous (multiple seconds); Apple does not document the exact value. 250 ms is a good global default; raise it to ~1 s for known-slow apps (Office) if you detect repeated `kAXErrorCannotComplete`.
3. **Use a serial queue per target PID** so you don't stack 20 pending calls against one app.
4. **Swift 6 concurrency:** `AXUIElement` is a CF type and **not `Sendable`**. Don't smuggle it across actors. Either confine all AX work to one actor/queue and pass value types (String, CGRect, CFRange) across boundaries, or wrap in a `final class @unchecked Sendable` box that owns the element and never escapes its queue.
5. **Error codes you will actually see** (from `AXError.h`): `kAXErrorAPIDisabled` (-25211, not trusted), `kAXErrorCannotComplete` (-25204, target hung/not responding), `kAXErrorAttributeUnsupported` (-25205), `kAXErrorInvalidUIElement` (-25202, element went away), `kAXErrorNotImplemented` (-25208), `kAXErrorNoValue` (-25212).

   **Live finding on macOS 26.6.2:** an untrusted process calling `kAXFocusedUIElementAttribute` on the system-wide element gets **-25204 (`kAXErrorCannotComplete`)**, *not* `kAXErrorAPIDisabled`. So **never infer permission state from error codes** — call `AXIsProcessTrusted()` / `AXIsProcessTrustedWithOptions` explicitly. (Verified by compiling and running a small Swift binary on this machine.)

6. **Notifications** (`AXObserverCreate`, `kAXFocusedUIElementChangedNotification`, `kAXSelectedTextChangedNotification`, `kAXValueChangedNotification`) are how you avoid polling. Reality: AppKit apps are reliable; Chromium posts selection changes but **not** on every keystroke in every editor. Plan a **hybrid**: observer + a 150–250 ms debounce timer that re-reads the focused element while a text field has focus, and a `NSWorkspace.didActivateApplicationNotification` observer to reset state on app switch.

---

## 2. Permissions and entitlements

### 2.1 The TCC services that matter

| Service | TCC name | What triggers the prompt | Needed for |
|---|---|---|---|
| **Accessibility** | `kTCCServiceAccessibility` | First `AXUIElement*` call against another process, or explicit `AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt: true])` | Reading/injecting into other apps' UI; posting synthetic CGEvents |
| **Input Monitoring** | `kTCCServiceListenEvent` | `CGEventTapCreate` (listen-only) or `IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)` | Observing keystrokes globally (hotkeys via tap, selection-change detection) |
| **Screen Recording** | `kTCCServiceScreenCapture` | First `SCStream`, `CGWindowListCreateImage`, `CGDisplayStream`, or `CGRequestScreenCaptureAccess()` | OCR fallback, screenshot-based capture |
| **Apple Events** | `kTCCServiceAppleEvents` | First `NSAppleScript`/`OSAScript` targeting another app | Scripting apps (Word/Pages), not needed for AX |
| **Post Event** | `kTCCServicePostEvent` | Synthetic event posting on newer macOS | **[unverified]** whether 26 splits this from Accessibility; assume Accessibility covers it |

Accessibility and Input Monitoring are **separate** and both appear as separate rows in System Settings → Privacy & Security. Users routinely grant one and not the other; your onboarding must show both states independently.

### 2.2 Code: prompting and deep-linking

```swift
import ApplicationServices

// Accessibility — prompt once, then poll for the user granting it
func requestAccessibility() -> Bool {
    let opts = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
    return AXIsProcessTrustedWithOptions(opts)
}

func isTrusted() -> Bool { AXIsProcessTrusted() }
```

```swift
// Deep links into the exact settings panes (stable for years, still work on 26)
let panes = [
  "accessibility": "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
  "inputMonitoring": "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
  "screenRecording": "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
  "automation": "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
  "loginItems": "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
]
NSWorkspace.shared.open(URL(string: panes["accessibility"]!)!)
```

Input Monitoring has a proper preflight API, which is much nicer than guessing:

```swift
import IOKit.hid
let status = IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) // .granted/.denied/.unknown
if status == .unknown { IOHIDRequestAccess(kIOHIDRequestTypeListenEvent) }
```

Screen Recording likewise:

```swift
CGPreflightScreenCaptureAccess()          // Bool, no prompt
CGRequestScreenCaptureAccess()            // prompts once
```

**UX notes that matter in practice:**
* After the user toggles Accessibility, the grant takes effect for **new** AX calls without restarting your app (the daemon checks per-connection), but some apps need a relaunch to clear cached failures. Offer a "Restart app" button.
* The prompt is a **modal system alert**; if you trigger it from a background agent with no UI, users may not see it. Always explain *why* in your own window first, then call the prompt.
* Detect revocation at runtime: poll `AXIsProcessTrusted()` every few seconds while active, and immediately hide overlays on false.

### 2.3 TCC identity: why code signing is a correctness issue

TCC stores grants keyed to the app's **designated requirement** (bundle identifier + Team ID + signing identity), not its path. Consequences:

* **Ad-hoc signed dev builds** ("Sign to Run Locally") get a new cdhash every build. Depending on macOS version and the specific service, this causes **re-prompts on every rebuild** — the single most annoying part of developing this class of app. **Fix: create a persistent self-signed certificate in Keychain Access and sign dev builds with it**, with a stable bundle ID.
* **Changing your Team ID or bundle ID orphans the grant.** Users see "the app you previously granted is gone" and must re-add it. Never change either after 1.0.
* **Moving/renaming the .app** generally preserves the grant because identity isn't path-based — but replacing it with a differently-signed build does not.
* `tccutil reset Accessibility <bundle-id>` is the clean way to test first-run flow.

### 2.4 Sandboxed (MAS) vs Developer ID + notarized

| | **Mac App Store (App Sandbox)** | **Developer ID + notarized, direct** |
|---|---|---|
| AX access to other apps | **No.** The sandbox blocks the connection to the accessibility server for other processes; `AXIsProcessTrusted()` never becomes true for cross-app use | **Yes**, with user-granted Accessibility |
| Synthetic events to other apps | **No** | Yes |
| Screen capture | Only via the sandbox-compatible ScreenCaptureKit path with user consent | Yes |
| Distribution friction | Low for users | Gatekeeper/notarization, but one DMG |
| Review risk | High (Apps that "modify other apps' text" attract scrutiny) | None |

**Answer to the direct question: no, a sandboxed Mac App Store app cannot read other apps' text through Accessibility.** The App Sandbox denies the mach connection to the AX server; the API returns errors and the app never appears as a trusted client. This is why Grammarly, Raycast, PopClip, TextSniper and every comparable tool ship **outside** the Mac App Store. **[High confidence — consistent across Apple's sandbox model and every shipping tool in this category; the definitive statement is not in a single Apple doc, so validate with a 30-minute sandboxed spike before you stake the roadmap on it.]**

**What this means architecturally:** if you also want a Safari extension, the Safari extension must ride inside the same **Developer ID** app (§6.5). You cannot split: MAS app for Safari + separate helper for AX, because the helper would still need to be installed and trusted separately, and MAS review will not accept a background helper that reads other apps.

### 2.5 macOS 15 / 26 specifics

* **Screen Recording re-prompts.** Starting with macOS 15 Sequoia, the system periodically re-asks whether an app may keep recording the screen (initially reported as weekly in betas, settling into a recurring/monthly cadence), and the purple menu-bar indicator persists during capture. **[unverified for 26 — I could not confirm the exact cadence without triggering it; budget for "user must re-approve periodically" and design the OCR path to degrade gracefully.]** Practical impact: **do not make OCR a hot path**. If your core loop needs Screen Recording, you inherit recurring consent fatigue.
* **Accessibility has not (yet) gained monthly re-prompts** — it remains a one-time grant per signature. This asymmetry is a strong argument for making AX the primary path.
* **macOS 26 (Tahoe) design changes:** AppKit now exposes `NSGlassEffectView` / `NSGlassEffectContainerView` (`API_AVAILABLE(macos(26.0))`, `NSGlassEffectViewStyleRegular|Clear`, `cornerRadius`, `tintColor`) — verified in the local SDK. This is the sanctioned way to make a suggestion card match Tahoe's Liquid Glass look; on 14/15 fall back to `NSVisualEffectView`. The macOS 26 menu bar is also translucent/adaptive, so **do not hard-code menu bar colors** for your status item; use template images. **[Menu-bar translucency behavior unverified in detail; template images are the safe choice regardless.]**
* **New AX attributes in 26** (§1.3) are the only AX-surface change I could confirm from headers.

---

## 3. Text injection / replacement

This is where most implementations die. Summary up front:

> **There is no API that reliably replaces text in an arbitrary focused field.** Every shipping tool uses a **ladder**: try AX, verify by reading back, fall back to pasteboard+⌘V, verify again, fall back to per-character key events, and if all fail, tell the user to copy/paste manually.

### 3.1 (a) `AXUIElementSetAttributeValue`

**How:** set `kAXValueAttribute` to the new full string (whole-field replace), or set `kAXSelectedTextRangeAttribute` to select, then set `kAXValueAttribute` — no, that's wrong: setting `AXValue` replaces the *entire* field, not the selection. To replace just the selection via AX you must read the full value, splice the string yourself, and write the whole value back.

```swift
func replaceViaAX(_ el: AXUIElement, range: CFRange, with newText: String) -> Bool {
    guard var full = value(of: el) else { return false }
    let ns = full as NSString
    guard range.location >= 0, range.location + range.length <= ns.length else { return false }
    let updated = ns.replacingCharacters(in: range, with: newText)
    let ok = AXUIElementSetAttributeValue(el, kAXValueAttribute as CFString, updated as CFString) == .success
    if ok {
        var caret = CFRange(location: range.location + (newText as NSString).length, length: 0)
        if let v = AXValueCreate(.cfRange, &caret) {
            AXUIElementSetAttributeValue(el, kAXSelectedTextRangeAttribute as CFString, v)
        }
    }
    return ok
}
```

| Property | Assessment |
|---|---|
| Reliability | **High in AppKit**, **zero in Chromium/Electron** (no setter branch — §1.4) |
| Latency | ~1–5 ms |
| Focus | **None needed** — works while your panel is key, which is a huge UX advantage |
| Undo | **Poor.** Whole-field replace usually registers as one giant undo step or none at all; user's ⌘Z may wipe their paragraph |
| Corruption risk | **High** if you read a stale value; concurrent typing during the read→splice→write window loses keystrokes |
| Rich text | Destroys attributes (bold/links) unless you write an attributed string |
| Secure fields | Refused by the app |

**When to use:** AppKit apps where `AXUIElementIsAttributeSettable(el, kAXValueAttribute)` is true **and** the field is short **and** the app is not Chromium. This is the "fast path" for Notes/Mail/TextEdit-class apps.

### 3.2 (b) `CGEvent` keyboard synthesis

Two sub-techniques, with very different reliability.

**(b1) Unicode string events**

```swift
let src = CGEventSource(stateID: .combinedSessionState)
src?.setLocalEventsFilterDuringSuppressionState(
    [.permitLocalMouseEvents, .permitLocalKeyboardEvents],
    state: .eventSuppressionStateSuppressionInterval)

for chunk in text.chunked(into: 20) {           // keep each event small
    let down = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true)
    down?.keyboardSetUnicodeString(stringLength: chunk.utf16.count,
                                   unicodeString: Array(chunk.utf16))
    down?.post(tap: .cghidEventTap)
    let up = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: false)
    up?.keyboardSetUnicodeString(stringLength: chunk.utf16.count,
                                 unicodeString: Array(chunk.utf16))
    up?.post(tap: .cghidEventTap)
}
```

**The header tells you exactly why this is unreliable:**

> "By default, the system translates the virtual key code in a keyboard event into a Unicode string… This function allows you to manually override this string. **Note that application frameworks may ignore the Unicode string in a keyboard event and do their own translation based on the virtual keycode and perceived event state.**"
> — `CGEvent.h`, `CGEventKeyboardSetUnicodeString`

Chromium/Electron do exactly this in some paths. So unicode-string typing is fine for AppKit, flaky for browsers. It also **bypasses IME composition** — for CJK input methods this can produce garbage or commit half-composed text.

**(b2) Pasteboard + ⌘V** — the de facto standard.

```swift
func paste(_ text: String) {
    let pb = NSPasteboard.general
    let saved = pb.pasteboardItems?.compactMap { $0.copy() as? NSPasteboardItem } ?? []
    let before = pb.changeCount

    pb.clearContents()
    let item = NSPasteboardItem()
    item.setString(text, forType: .string)
    // Tell clipboard managers to ignore it (nspasteboard.org conventions)
    item.setString("", forType: NSPasteboard.PasteboardType("org.nspasteboard.TransientType"))
    pb.writeObjects([item])

    synthesizeCommandV()

    // Restore after the target has consumed it, and only if nobody else wrote
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.20) {
        guard pb.changeCount == before + 1 else { return }
        pb.clearContents()
        pb.writeObjects(saved)
    }
}
```

`synthesizeCommandV()` posts keycode 9 (`kVK_ANSI_V`) with `.maskCommand` down/up to `.cghidEventTap` (or `CGEventPostToPid(pid, event)` if you want to target a specific app — the latter still requires Accessibility and is not honored by all apps).

| Property | Assessment |
|---|---|
| Reliability | **Highest of all options** — every editor handles ⌘V, including Chromium, Electron, terminals (bracketed paste), Office |
| Latency | ~30–80 ms (dominated by the restore delay) |
| Focus | **Requires the target app to be frontmost and the field focused.** Your panel must not steal focus |
| Undo | **Good** — one undo group in most apps; ⌘Z restores the previous text |
| Corruption risk | Clipboard restore races with clipboard managers; a slow/hung target may paste *after* you restored, yielding old content |
| Side effects | Fires "paste" in apps that log it; some apps sanitize pasted text (Word "keep text only"); rich-text paste may carry your styled attributes unless you paste as plain text |
| Secure input | ⌘V into a password field is blocked by the app |

Mitigations that actually matter: mark the item transient + concealed so Maccy/Raycast clipboard history ignores it; **restore by item, not by string**; verify by re-reading the field afterwards; and if the restore's `changeCount` check fails, **don't fight the user's clipboard manager** — just leave it.

### 3.3 (c) InputMethodKit custom input method

An IME (`IMKInputController`, installed as an `.inputmethod` bundle in `~/Library/Input Methods`) sees **every keystroke destined for any text field**, can compose/replace text, and can render a candidate window — all without Accessibility permission for *reading* keystrokes.

Why it's attractive: no TCC prompt for input, perfect knowledge of the text being typed, and replacement is native (you own the composition).

Why it's usually rejected:
* The user must **add and switch to your input source**; it applies to *all* typing, so any bug breaks typing everywhere.
* You do **not** get the surrounding document — only what passes through the IME. Reading existing text still needs AX.
* You must coexist with the user's real IME (Japanese/Chinese/Vietnamese users will hit conflicts).
* Installing an IME from a non-MAS app triggers Gatekeeper warnings and requires a logout/login cycle for registration.
* Apple's InputMethodKit has been effectively frozen for years. **[unverified for 26]** — no evidence of new investment.

**Verdict: not for MVP, not for v1.** It's the right answer only if you decide to become a full writing environment.

### 3.4 (d) `NSPasteboard` + Services / Sharing

A **Service** (`NSServices` in Info.plist, `@objc` method taking `NSPasteboard`) appears in the contextual menu of text fields in most apps (Services menu / right-click → Services). It can **read the selection and return a replacement** — the system handles the replacement, including undo.

* Pros: no Accessibility permission, zero injection risk, native undo, works in Chromium.
* Cons: requires a right-click → Services trip (two clicks + a menu hunt), no inline UI, no automatic triggering, and the Services menu is being de-emphasized in modern macOS. Good as a **fallback/accessibility affordance**, not a primary path.

### 3.5 Comparison matrix

| | AX set | CGEvent unicode | Pasteboard+⌘V | IMKit IME | Services |
|---|---|---|---|---|---|
| AppKit | ✅ best | ✅ | ✅ | ✅ | ✅ |
| Chromium/Electron | ❌ no-op | ⚠️ often ignored | ✅ | ✅ | ✅ |
| Terminal | ❌ | ⚠️ | ⚠️ (bracketed paste) | ✅ | ❌ |
| Needs frontmost | ❌ no | ✅ yes | ✅ yes | n/a | ❌ |
| Undo behaves | ❌ | ✅ | ✅ | ✅ | ✅ |
| Latency | ~2 ms | ~5 ms/char | 30–80 ms | ~0 | user-driven |
| Text corruption risk | **high** | medium | medium (clipboard race) | low | low |
| Permissions | Accessibility | Accessibility | Accessibility | Input source install | none |
| Rich text preserved | ❌ | ❌ | ⚠️ (plain-text only if you paste as string) | ❌ | ✅ |

**What Grammarly et al. reportedly do [unverified — no public source found this session]:** the widely-held model is AX for read + pasteboard/keystroke for write, with a per-app compatibility list (Grammarly maintains an explicit supported-apps list and disables itself in unsupported ones). Writing Tools, which *is* open source, documents the same approach: Accessibility "to read/replace selected text", and notes **"for some apps, Screen Recording is also required"** — i.e. an OCR fallback exists for apps that expose nothing ([Writing Tools README](https://github.com/theJayTea/WritingTools)).

### 3.6 Pitfalls checklist

* **Selection replacement vs whole field** — decide explicitly per app. Whole-field replace in a 5,000-word doc is unacceptable even when it works.
* **Caret/selection restoration** — after any replacement, set `kAXSelectedTextRangeAttribute` to `location + insertedLength`, or the caret jumps to the start of the field and the user's next keystroke lands in the wrong place.
* **Stale reads** — always re-read the field immediately before injecting; compare against the text you analyzed. If it changed, **abort** rather than apply a stale correction.
* **IME composition** — if the user is mid-composition (Japanese/Chinese/Korean), never inject. Heuristic: if `AXValue` ends with unconverted kana or the app exposes a marked-text range, back off. **[Marked-text AX attribute availability unverified.]**
* **Autocorrect/spellcheck interference** — AppKit may autocorrect your inserted text or show a red squiggle; there is no API to disable another app's autocorrect.
* **Electron/VS Code** — VS Code's editor is canvas-ish (Monaco), and its AX exposes the *whole buffer* with a selection range; replacing via AX is impossible, paste works, but the editor may reformat on paste. Consider **shipping a VS Code extension** instead for that audience.
* **Terminal** — do not auto-inject; a stray newline executes a command. If you must, paste with bracketed-paste handling and never include `\n` unless the user asked.
* **Password fields** — check `AXSubrole == "AXSecureTextField"` and `IsSecureEventInputEnabled()` (declared in `Carbon/HIToolkit/CarbonEventsCore.h`, verified in SDK) and **disable yourself entirely** when true.
* **Clipboard managers** — Maccy/Raycast will capture your replacement text unless you tag it transient/concealed.
* **Rate limits** — some apps throttle AX writes; batch edits per field.

---

## 4. Overlay / non-activating UI

### 4.1 The panel recipe

The goal: a card that appears next to the caret, **never becomes key**, and lets clicks through to its buttons without bringing your app forward.

```swift
final class SuggestionPanel: NSPanel {
    init(content: NSView) {
        super.init(contentRect: .zero,
                   styleMask: [.nonactivatingPanel, .borderless, .fullSizeContentView],
                   backing: .buffered,
                   defer: false)
        isFloatingPanel = true
        becomesKeyOnlyIfNeeded = true      // NSPanel property, verified in NSPanel.h
        worksWhenModal = true
        level = .floating                  // NSWindow.Level.floating == CGLevel 3
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        hidesOnDeactivate = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        isMovableByWindowBackground = false
        animationBehavior = .utilityWindow
        contentView = content
    }
    override var canBecomeKey: Bool { false }   // never take key focus
    override var canBecomeMain: Bool { false }
}
```

Key facts verified in the macOS 26 SDK:

* `.nonactivatingPanel` exists as `NSWindowStyleMaskNonactivatingPanel` and is documented as *"The window is a panel or a subclass of `NSPanel` that does not activate the owning app."*
* `NSPanel.becomesKeyOnlyIfNeeded` and `worksWhenModal` are public properties (`NSPanel.h`).
* Window levels (`CGWindowLevel.h`): `normal 0`, `floating 3`, `modalPanel 8`, `utility 19`, `mainMenu 24`, `status 25`, `popUpMenu 101`, `overlay 102`, `help 200`, `screenSaver 1000`, **`assistiveTechHigh 1500`**.
  → Use `.floating` (3) for the card so it sits above normal windows but below menus; use **`CGWindowLevel(1500)`** for inline underline overlays if you need them above full-screen apps' chrome (this is the level Accessibility Inspector uses).
* Collection behaviors and their exact semantics: `.canJoinAllSpaces` ("appears in all spaces"), `.moveToActiveSpace`, `.fullScreenAuxiliary` ("displays on the same space as the full screen window"), `.transient` ("floats in Spaces and hides in Mission Control"), `.stationary`, and — since macOS 13 — the Stage Manager trio `.primary`, `.auxiliary`, `.canJoinAllApplications` ("marking this window as one that can join all apps for both Stage Manager and full screen"). For an assistant card you want `.canJoinAllSpaces + .fullScreenAuxiliary + .transient`; `.canJoinAllApplications` is the one that lets it follow *other apps'* Stage Manager sets.

**Crucial nuance: non-activating ≠ key-capable.** If `canBecomeKey` is false, your card can't host a text field (search box, custom instruction input). If you *do* need typing, you must either (a) let it become key and then **restore focus** to the previous app afterwards (`NSRunningApplication(processIdentifier: pid)?.activate()` plus re-setting `kAXFocusedAttribute`), or (b) capture keystrokes globally with a CGEvent tap while it's visible (requires Input Monitoring; risky). **For MVP: click-only card, no text input, no focus stealing.** This is exactly the Grammarly card model.

### 4.2 Coordinate conversion (the part everyone gets wrong)

AX gives you rects in the **global display space**: origin at the **top-left of the primary display**, y increasing downward, units = points (Retina-independent).

Cocoa (`NSWindow` frames, `NSScreen.frame`) uses origin at the **bottom-left of the primary display**, y increasing upward.

```swift
/// Convert an AX (top-left origin, global) rect to Cocoa screen coordinates.
func cocoaRect(fromAX r: CGRect) -> NSRect {
    guard let primary = NSScreen.screens.first else { return .zero }
    let h = primary.frame.height
    return NSRect(x: r.origin.x,
                  y: h - r.origin.y - r.height,
                  width: r.width,
                  height: r.height)
}
```

This works for **all** displays including ones positioned above or to the left of the primary (their AX y is negative, or > primary height). Do **not** try to convert per-screen using `NSScreen.frame` of the *containing* screen — that's the classic bug that misplaces cards on secondary monitors.

Placement heuristic:

```swift
func place(panel: NSPanel, near axRect: CGRect, caretHeight: CGFloat) {
    var r = cocoaRect(fromAX: axRect)
    let size = panel.frame.size
    let screen = NSScreen.screens.first { $0.frame.intersects(r) } ?? NSScreen.main!

    var origin = CGPoint(x: r.maxX + 8, y: r.minY)          // right of the field
    if origin.x + size.width > screen.visibleFrame.maxX {   // flip to left
        origin.x = r.minX - size.width - 8
    }
    if origin.y < screen.visibleFrame.minY {                // clamp above menu bar
        origin.y = r.maxY + 8
    }
    panel.setFrame(NSRect(origin: origin, size: size), display: true)
}
```

Retina: nothing to do — `AXBoundsForRange` returns points, and `NSWindow` frames are points. Do **not** multiply by `backingScaleFactor`. (You *do* multiply for pixel-level OCR work.)

### 4.3 Inline underlines over other apps' text

Two strategies:

**A. AX-driven (preferred).** For the paragraph/range you analyzed, split into sentences/words, call `AXBoundsForRange` per flagged span, and draw rounded rects in a **click-through overlay window**:

```swift
final class UnderlineOverlay: NSWindow {
    init(screen: NSScreen) {
        super.init(contentRect: screen.frame, styleMask: [.borderless],
                   backing: .buffered, defer: false)
        level = NSWindow.Level(rawValue: 1500)     // kCGAssistiveTechHighWindowLevel
        isOpaque = false
        backgroundColor = .clear
        ignoresMouseEvents = true                   // click-through
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        hasShadow = false
        sharingType = .none                         // keep it out of captures where possible
    }
    override var canBecomeKey: Bool { false }
}
```

Draw with a layer-backed `NSView` and **disable implicit animations**, which is the #1 flicker source:

```swift
CATransaction.begin()
CATransaction.setDisableActions(true)
layer.sublayers?.forEach { $0.removeFromSuperlayer() }
for r in rects { /* add CAShapeLayer with rounded path */ }
CATransaction.commit()
```

**B. OCR-driven (fallback).** `SCScreenshotManager.captureImage` (ScreenCaptureKit) or `CGWindowListCreateImage`, then `VNRecognizeTextRequest` with `.accurate`, then map `VNRecognizedTextObservation.boundingBox` (normalized, bottom-left origin) back to screen coords. You get **no character-level positions**, so underlines are per-line at best, and you pay Screen Recording consent. Keep this strictly opt-in.

Flicker/robustness rules:
* One overlay window **per screen**, created once, hidden/shown — never recreated per suggestion.
* Hide on: app switch (`NSWorkspace.didActivateApplicationNotification`), space change (`activeSpaceDidChangeNotification`), screen lock, `AXUIElement` invalidation, and window move (`kAXWindowMovedNotification`).
* Reposition on scroll: `kAXBoundsForRange` again after a debounce; if the field scrolled out of view, hide.
* Compare-and-skip: if the new rect set equals the old within 0.5 pt, don't redraw.

### 4.4 Spaces, Stage Manager, full screen, multi-monitor

* **Spaces:** `.canJoinAllSpaces` is required or the card vanishes when the user switches desktops. `.moveToActiveSpace` is the alternative if you want it to follow only the active space.
* **Full screen:** `.fullScreenAuxiliary` lets your panel appear alongside another app's full-screen window. Without it, your panel simply isn't shown. This is essential — much writing happens in full-screen browsers.
* **Stage Manager:** Stage Manager moves non-active apps' windows off to the side; AX frames of a backgrounded window are not where you think. Since macOS 13, `.auxiliary` / `.canJoinAllApplications` let a window join the active app's set. Practically: **only show the card for the frontmost app** and hide it when Stage Manager changes the layout (observe `NSWorkspace.activeSpaceDidChangeNotification` and `didActivateApplicationNotification`).
* **Multi-monitor:** the AX global space spans all displays; use §4.2. Watch `NSApplication.didChangeScreenParametersNotification` and rebuild per-screen overlays.
* **Menu bar / notch:** `NSScreen.visibleFrame` excludes the menu bar; on notched Macs `auxiliaryTopLeftArea` matters if you place cards above the field.
* **Screen recording implications for overlays:** your own windows do **not** require Screen Recording. But `sharingType = .none` is ignored by ScreenCaptureKit-based capture, and the window *will* appear in the user's screen share unless you exclude it via `SCContentFilter(desktopIndependentWindow:)` when *you* are capturing. If the user shares their screen, your underlines show up. Consider a "hide overlays while screen sharing" preference by observing `NSWorkspace.shared.isScreenCaptureActive`-style signals. **[API availability unverified.]**

---

## 5. Background lifecycle

### 5.1 Menu bar agent

```xml
<!-- Info.plist -->
<key>LSUIElement</key><true/>
```

`LSUIElement` is documented as *"A Boolean value indicating whether the app is an agent app that runs in the background and doesn't appear in the Dock"* (verified in Apple's Information Property List docs). Belt-and-braces at runtime:

```swift
NSApp.setActivationPolicy(.accessory)
```

Two UI options:

* **`NSStatusItem`** (AppKit) — full control, custom `NSStatusItem.button` views, precise highlighting. Use **template images** so it adapts to light/dark and macOS 26's translucent menu bar.
* **`MenuBarExtra`** (SwiftUI, macOS 13+) — declarative, less control; `.menuBarExtraStyle(.window)` gives a popover-like panel. Verified present in the local SDK's `SwiftUI.swiftinterface`.

For a Grammarly-class app, `NSStatusItem` is the safer choice (MenuBarExtra has had focus/ordering quirks with `LSUIElement` apps). **[unverified for 26.]**

### 5.2 Launch at login

```swift
import ServiceManagement

let service = SMAppService.mainApp           // macOS 13+
try? service.register()
// status: .enabled / .requiresApproval / .notRegistered / .notFound
```

Apple's docs frame `SMAppService` as *"An object the framework uses to control helper executables that live inside an app's main bundle"*, and explicitly as the replacement for `SMLoginItemSetEnabled` for `LoginItems`. Two operational notes:

* The app must be in `/Applications` (or at least a stable location) for reliable registration; running from Xcode's DerivedData will register a path that later disappears.
* `.requiresApproval` means the user must approve in System Settings → General → Login Items; deep-link to it (§2.2) and show the state in your settings UI.

### 5.3 Sparkle 2 auto-update

* EdDSA signing: generate a key pair with Sparkle's `generate_keys`; put the **public** key in `Info.plist` as `SUPublicEDKey`; sign each release with `sign_update` producing `sparkle:edSignature` in the appcast.
* Appcast: `appcast.xml` with `<item>` per version, `sparkle:version`, `sparkle:shortVersionString`, `sparkle:minimumSystemVersion`, enclosure URL + length + signature.
* Delta updates: `generate_appcast` emits `.delta` files; clients fetch only the diff. Requires keeping prior releases' archives around.
* Embed `Sparkle.framework` with **Embed & Sign**; hardened runtime is compatible; sandboxed apps need XPC entitlements (another reason not to sandbox).
* Serve the appcast over HTTPS; Sparkle refuses plain HTTP by default. `SUFeedURL` in Info.plist.
* Docs: [sparkle-project.org/documentation](https://sparkle-project.org/documentation/).

### 5.4 Signing, notarization, packaging

```bash
# Universal binary (Apple Silicon + Intel). Xcode 26: set ARCHS="arm64 x86_64", ONLY_ACTIVE_ARCH=NO
xcodebuild -scheme GrammarlyLike -configuration Release \
  ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO \
  CODE_SIGN_IDENTITY="Developer ID Application: You (TEAMID)" \
  CODE_SIGN_STYLE=Manual OTHER_CODE_SIGN_FLAGS="--timestamp --options runtime" \
  build

# Verify
codesign -dv --verbose=4 App.app        # expect: flags=0x10000(runtime), TeamIdentifier
spctl -a -vvv -t exec App.app

# Notarize
ditto -c -k --keepParent App.app App.zip
xcrun notarytool submit App.zip --apple-id you@example.com --team-id TEAMID \
  --password "app-specific-password" --wait
xcrun stapler staple App.app
```

Hardened runtime entitlements you may need:
* None for AX/TCC (grants are user-level, not entitlement-based).
* `com.apple.security.automation.apple-events` **only** if you send Apple Events.
* `com.apple.security.cs.disable-library-validation` **only** if loading unsigned plugins — **Sparkle does not require it**; don't add it (it weakens the runtime and draws scrutiny).
* `com.apple.security.cs.allow-jit` only if you embed a JS engine.

DMG: `create-dmg` or `hdiutil create -volname ... -srcfolder App.app -ov -format UDZO out.dmg`; sign the DMG (`codesign --sign "Developer ID Application: ..." out.dmg`) and notarize the DMG too (staple the DMG, and staple the .app inside before building the DMG).

**Homebrew Cask:** submit a PR to `Homebrew/homebrew-cask` with a `cask` file pointing at your DMG URL, `sha256`, `app "Name.app"`, `zap trash: [...]`. Requirements: notarized, versioned URL, no auto-update conflicts (Sparkle is fine; add `auto_updates true`). Livecheck must work (GitHub releases are easiest).

### 5.5 macOS 26 (Tahoe) notes

* **`NSGlassEffectView` / `NSGlassEffectContainerView`** (macOS 26.0) for Liquid Glass cards — verified in the SDK. Use `NSVisualEffectView` on 14/15.
* **Menu bar is translucent/adaptive.** Template images, no hard-coded colors, and test with a light wallpaper.
* **Background-item management** in System Settings (Login Items & Extensions) lists your login item; users can disable it and you'll see `.requiresApproval`/`.notRegistered`. **[Behavior details unverified.]**
* Everything else in the AX/TCC/injection stack appears unchanged in 26 — the only AX-surface delta I could confirm from headers is the new attributed-string keys (§1.3).

---

## 6. Browser extension path

### 6.1 Manifest V3 skeleton

```json
{
  "manifest_version": 3,
  "name": "Writing Assistant",
  "version": "0.1.0",
  "permissions": ["storage", "scripting", "activeTab", "nativeMessaging"],
  "host_permissions": ["<all_urls>"],
  "background": { "service_worker": "sw.js", "type": "module" },
  "content_scripts": [{
    "matches": ["<all_urls>"],
    "js": ["content.js"],
    "css": ["content.css"],
    "run_at": "document_idle",
    "all_frames": true
  }],
  "web_accessible_resources": [{ "resources": ["ui.html"], "matches": ["<all_urls>"] }]
}
```

MV3 realities that bite:
* **The service worker is ephemeral** (killed after ~30 s idle). Keep no state there; persist in `chrome.storage.session` and re-connect on demand.
* **No remote code.** All logic ships in the bundle; your "AI rules" must be data, not fetched JS.
* `all_frames: true` is mandatory — Gmail, Notion, and many editors put the editable surface in an iframe.
* Content scripts share the page's DOM but not its JS context; for editors with complex JS state, use `chrome.scripting.executeScript({world: "MAIN"})` when you must touch framework internals.

### 6.2 Editor support matrix

| Target | Editable surface | Read | Replace | Decorate | Notes |
|---|---|---|---|---|---|
| `<textarea>` / `<input>` | native | ✅ `.value` | ✅ set `.value` + `input` event | ❌ (cannot style text) | Overlay a `<div>` mirror to fake underlines |
| `contenteditable` | DOM | ✅ Range/Selection | ✅ `Range.deleteContents` + `insertNode` | ✅ Range + CSS overlay | Fire `beforeinput`/`input` so React/Vue state updates |
| **Gmail** | `div[contenteditable]` (body) | ✅ | ⚠️ | ✅ | Gmail's own undo stack fights you; dispatch `InputEvent` with `inputType: "insertText"` |
| **Notion** | nested `contenteditable` blocks | ✅ (fragile) | ⚠️ | ⚠️ | Virtualized blocks; selection can be re-rendered mid-flight |
| **Slack web** | `contenteditable` (Slate-like) | ✅ | ⚠️ | ✅ | Quill/Slate need proper `beforeinput` |
| **Google Docs** | **canvas** | ❌ DOM | ❌ | ❌ | See §1.5 — needs Add-on or API |
| VS Code web / Monaco | canvas-ish + hidden textarea | ⚠️ | ⚠️ | ❌ | Ship a VS Code extension instead |
| CodeMirror 6 | DOM lines + hidden input | ✅ | ⚠️ | ✅ | `view.dispatch` if you can reach the instance |

**The reliable pattern for contenteditable replacement:**

```js
function replaceRange(range, text) {
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
  const ok = document.execCommand('insertText', false, text); // still the most compatible
  if (!ok) {
    range.deleteContents();
    range.insertNode(document.createTextNode(text));
    // then dispatch a synthetic InputEvent so frameworks notice
  }
}
```

`document.execCommand('insertText')` is deprecated but remains the **only** way to get a single undo entry and correct framework notifications in Chrome/Safari. Keep it, with a DOM fallback.

### 6.3 Drawing inline decorations

Three layers of increasing robustness:

1. **CSS Custom Highlight API** (`CSS.highlights`, `::highlight(name)`) — no DOM mutation, works on text nodes, ideal for underlines. Chrome 105+, Safari 17.2+. **Best choice in 2026.**
2. **Overlay layer** — an absolutely-positioned `<div>` in a shadow root appended to `document.documentElement`, with rects from `Range.getClientRects()`. Works everywhere, survives re-renders, and lets you draw cards/tooltips. This is what most commercial extensions do.
3. **Inline `<span>` wrapping** — most invasive; breaks editors that diff the DOM; avoid.

Use a **closed shadow root** for your overlay so page CSS can't touch it, and listen to `scroll`/`resize` (capture phase) plus `ResizeObserver` on the editable to keep rects fresh.

### 6.4 Native messaging vs local WebSocket

| | **Native messaging** | **Localhost WebSocket** |
|---|---|---|
| Transport | stdin/stdout, 4-byte length-prefixed JSON | `ws://127.0.0.1:PORT` |
| Registration | Host manifest JSON per browser, per user, in a fixed directory; `allowed_origins` must list the extension ID | None — just connect |
| Launch | Browser **spawns** the host process on demand | Your menu-bar app must already be running |
| Security | Browser-enforced allowlist, no network surface | Must implement token auth + `Origin` check + bind to loopback only |
| Multi-browser | Chrome, Edge, Brave, Arc, Firefox each need their own manifest path (and for Chrome, the extension ID changes if you repackage) | One server serves all browsers |
| Debugging | Awkward (stdio) | Easy (wscat, DevTools) |
| Safari | Different mechanism entirely: `SafariWebExtensionHandler` in the containing app (no manifest file) | Same WebSocket works |

Manifest locations (macOS), for the installer to write:
* Chrome: `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/<name>.json`
* Edge: `~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/`
* Brave: `~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/`
* Firefox: `~/Library/Application Support/Mozilla/NativeMessagingHosts/`
* Safari: no manifest — implement `SafariWebExtensionHandler` (see Apple's Safari web extension docs).

**Recommendation: ship both, in this order.** Start with **localhost WebSocket + bearer token** because it's one code path for all browsers and trivial to debug; add **native messaging** in v1 for the "works even when the app isn't running" property and because enterprise users' security teams prefer it. Note the architectural trap: with native messaging, the host process is a **child of the browser**, so it must relay to your menu-bar app over XPC or a Unix socket — don't duplicate the engine inside the host.

### 6.5 Safari Web Extension packaging and distribution

Apple's model (from the [Safari web extensions docs](https://developer.apple.com/documentation/safariservices/safari-web-extensions)):

> "You implement a Safari web extension as a macOS, visionOS, or iOS app extension to provide a safe and secure distribution and usage model. You can distribute a Safari web extension with a Mac app… Use `safari-web-extension-converter` to package your extension for testing and distribution…"

Facts that shape the plan:
* Safari 14+ on macOS; **Safari web extensions also run in Mac web apps on macOS 15+** (useful: your extension works in "Add to Dock" web apps).
* Dev loop: `xcrun safari-web-extension-converter <extension-dir>` generates an Xcode project; then enable **Develop → Allow Unsigned Extensions** in Safari (resets every launch) to run an unsigned build.
* Distribution: **either** Mac App Store **or** Developer ID-signed + notarized app. Both are supported. Given §2.4 (your main app can't be sandboxed), **Developer ID is the only coherent choice**: one notarized app containing both the menu-bar agent and the Safari extension.
* Safari's JS API surface differs slightly from Chrome's (`browser.*` vs `chrome.*`, promise-based). Use a thin polyfill or `webextension-polyfill`.

### 6.6 What a hobbyist can actually ship

* **Chrome/Edge/Arc (unpacked)**: instant, free. Chrome Web Store requires a $5 developer account; Edge store is free.
* **Safari**: requires the Apple Developer Program ($99/yr) for a Developer ID cert **and** for notarization. Without it you can still run locally with Allow Unsigned Extensions, but you cannot distribute.
* **Native app**: Developer ID + notarization also requires the $99 program. There is no free path to a distributable, TCC-stable macOS app.
* **Minimum viable hobby release:** unsigned/ad-hoc local build + unpacked Chrome extension, clearly labeled "dev build". Anything user-facing needs the $99.

---

## 7. Prior art

| Project | What it does | Approach / permissions | Link |
|---|---|---|---|
| **Writing Tools** (open source, Swift port) | System-wide proofreading/rewriting via Apple Intelligence or LLM; "does not mess with your clipboard" | **Accessibility** to read/replace selected text; **Screen Recording** required for some apps (OCR fallback). Explicitly documents both prompts on first launch | [github.com/theJayTea/WritingTools](https://github.com/theJayTea/WritingTools) |
| **PopClip** | Selection popover with actions (translate, AI, etc.) | AX to detect selection + simulate ⌘C; non-activating panel; **Accessibility** permission; huge extension ecosystem | [popclip.app](https://www.popclip.app/) · [extension SDK](https://www.popclip.app/dev/) |
| **Espanso** | Cross-platform text expander (Rust) | Global keyboard hook; injects via synthetic events / clipboard; needs Accessibility (+ Input Monitoring on macOS) | [github.com/espanso/espanso](https://github.com/espanso/espanso) |
| **Hammerspoon** | Lua automation; `hs.axuielement` is the best-documented AX wrapper anywhere | AX for everything; ships `hs.axuielement.axtextmarker`, parameterized-attribute tables, observers | [hammerspoon.org/docs/hs.axuielement.html](https://www.hammerspoon.org/docs/hs.axuielement.html) |
| **AXSwift / Swindler** | Swift AX wrappers | Thin, complete coverage of the C API; good reference for error handling and observer plumbing | [AXSwift](https://github.com/tmandry/AXSwift) · [Swindler](https://github.com/tmandry/Swindler) |
| **Ice** | Menu bar manager | Uses AX + ScreenCaptureKit to read/hide menu bar items; shows how to drive other apps' menu bar extras | [github.com/jordanbaird/Ice](https://github.com/jordanbaird/Ice) |
| **Rectangle** | Window manager | Pure AX positioning (`kAXPositionAttribute`/`kAXSizeAttribute`), Accessibility only; the cleanest reference for AX geometry | [github.com/rxhanson/Rectangle](https://github.com/rxhanson/Rectangle) |
| **TextSniper** | OCR any screen region → text | Screen Recording + Vision; no AX at all. Closed source, but the canonical OCR-only architecture | [textsniper.app](https://www.textsniper.app/) |
| **Raycast** | Launcher + AI commands | AX + clipboard; not open source; its "selected text" commands are the closest commercial analogue | [raycast.com](https://www.raycast.com/) |
| **Maccy** | Clipboard manager | Pasteboard polling, no AX; relevant because your clipboard-restore must not fight it | [github.com/p0deje/Maccy](https://github.com/p0deje/Maccy) |
| **Grammarly Desktop** | The product being cloned | Closed. Publicly maintains a supported-apps list and degrades gracefully elsewhere. Read+replace via AX/clipboard **[unverified]** | [grammarly.com/desktop](https://www.grammarly.com/desktop) |

The consistent pattern across all of them: **Accessibility + non-activating panel + pasteboard fallback**, with OCR only where AX fails, and an explicit supported-apps list rather than a promise of universality.

---

## 8. Recommendation

### 8.1 MVP (4–6 weeks, one engineer)

**Scope:** menu-bar agent, English-only suggestions, one suggestion at a time, no underlines, Chrome/Edge extension optional.

| Layer | Choice | Why |
|---|---|---|
| App type | `LSUIElement` agent + `NSStatusItem` | No Dock icon, always resident |
| Read | AX: focused element → role check → `AXSelectedText` if non-empty, else `AXValue` (or `AXStringForRange` around the caret) | Cheapest, no permissions beyond Accessibility |
| Fallback read | **None in MVP** (explicitly unsupported apps list) | OCR brings Screen Recording + consent fatigue |
| Write | **Pasteboard + ⌘V** with transient tagging + restore + read-back verification | Only near-universal mechanism |
| Fast path | AX `kAXValueAttribute` write **only** when `AXUIElementIsAttributeSettable` is true **and** role is a native text role **and** the app's bundle ID is not Chromium-family | 2 ms vs 60 ms, and preserves undo better in AppKit |
| Overlay | `NSPanel`, `.nonactivatingPanel`, `canBecomeKey = false`, `.floating` level, `[.canJoinAllSpaces, .fullScreenAuxiliary, .transient]` | Never steals focus |
| Trigger | Global hotkey (Carbon `RegisterEventHotKey`, no permission) + optional "check on pause" timer | Hotkey is deterministic and demo-friendly |
| Permissions flow | Accessibility only; explain → prompt → poll → deep-link → relaunch button | One permission is a shippable onboarding |
| Distribution | Developer ID + notarized DMG, Sparkle appcast | Required for TCC stability |
| Extension | None, or unpacked Chrome MV3 for textareas/contenteditable | Docs/Safari deferred |

**MVP non-goals:** Google Docs, Safari, OCR, inline underlines, per-app tuning, Teams/Slack desktop.

### 8.2 v1 (3–4 months)

Add, in this order (each is independently shippable):

1. **Inline underlines** for AppKit + Chromium via `AXBoundsForRange`, drawn in a click-through overlay at level 1500, with strict hide-on-switch rules.
2. **Chrome/Edge/Arc MV3 extension** with content scripts + CSS Custom Highlight API + localhost WebSocket pairing. This is *the* fix for the "browser is where people write" problem, and it eliminates AX fragility in browsers.
3. **Native messaging host** for robustness + auto-launch, once the WebSocket protocol is stable.
4. **Safari web extension** inside the same notarized app (needs `safari-web-extension-converter` + Developer ID).
5. **OCR fallback** (ScreenCaptureKit + Vision) behind an explicit "Enable for unsupported apps" toggle, accepting the Screen Recording consent and periodic re-prompt.
6. **Per-app compatibility matrix** with telemetry-free local diagnostics ("this app doesn't expose text — suggest the browser extension or manual copy").

### 8.3 Decision matrix

| Decision | Option A | Option B | **Recommendation** | Deciding factor |
|---|---|---|---|---|
| Distribution | MAS sandboxed | Developer ID notarized | **B** | Sandbox cannot do AX |
| Text read | AX | OCR | **AX first, OCR opt-in** | Consent fatigue + latency + caret mapping |
| Text write | AX set | Pasteboard+⌘V | **Pasteboard primary, AX fast path** | Chromium ignores AX writes |
| Write fallback | CGEvent unicode | Per-key keycodes | **Neither as primary** | Header says frameworks may ignore unicode strings |
| Overlay | Activating window | Non-activating panel | **B** | Focus stealing kills the product |
| Inline decoration | AX rects | OCR rects | **AX, OCR fallback** | Character-level accuracy |
| Browser comms | Native messaging | Local WS | **WS for MVP, NM for v1** | Debuggability vs robustness |
| Extension strategy | One cross-browser codebase | Per-browser | **One codebase + polyfill** | Cost |
| Docs support | Canvas OCR/Add-on | Declare unsupported | **Unsupported in v1** | Effort vs value |
| Sandbox helper | XPC helper sandboxed | None | **None** | Helper would still need AX |

### 8.4 Biggest failure modes (ranked)

1. **The write path silently corrupts text.** Mitigation: always read-back and verify; abort on mismatch; never write when the field changed since analysis; cap field size; keep a "undo" affordance by re-injecting the original text.
2. **Focus stealing.** One `makeKeyAndOrderFront` or `NSApp.activate` and the caret leaves the field, the ⌘V lands nowhere, and the user loses their place. Mitigation: `canBecomeKey = false`, no text input in the card, audit every window call.
3. **TCC churn during development** eats weeks. Mitigation: stable self-signed dev cert + fixed bundle ID from day one; a debug menu with `AXIsProcessTrusted` state, `tccutil reset` helper, and a permissions diagnostics screen.
4. **Chromium variability.** Chrome, Arc, Slack, VS Code, Notion all differ in what they expose and when. Mitigation: browser extension as the real answer; AX in browsers as best-effort only.
5. **Google Docs.** Users will ask on day one. Mitigation: explicit unsupported messaging + a "copy to our editor" path.
6. **Screen Recording consent fatigue** if you lead with OCR. Mitigation: never make OCR the default.
7. **Clipboard managers / password managers** racing your restore, and **secure input** silently blocking injection. Mitigation: transient+concealed pasteboard types; `IsSecureEventInputEnabled()` guard; disable entirely in secure fields.
8. **Coordinate bugs on multi-monitor/Spaces** making the card appear on the wrong screen. Mitigation: single `cocoaRect(fromAX:)` helper, unit-tested; hide on space change.

### 8.5 Unresolved questions to spike before committing

1. Does a **sandboxed** build truly fail all cross-app AX calls on 26? (30-minute spike; determines MAS viability.)
2. Does setting `AXEnhancedUserInterface`/`AXManualAccessibility` still force Chromium's a11y tree on 26? (Determines AX viability in browsers.)
3. Exact **Screen Recording re-prompt cadence** on 26, and whether `SCContentFilter` per-window capture avoids it.
4. Whether `kAXValueAttribute` writes in **SwiftUI `TextEditor`** preserve undo on 26.
5. Safari's `SafariWebExtensionHandler` performance for per-keystroke messaging.

---

## Appendix A — Verification log (what was actually checked)

| Claim | How verified |
|---|---|
| `AXUIElementCreateSystemWide`, `kAXFocusedUIElementAttribute`, `AXUIElementCopyAttributeValue` signatures | macOS 26 SDK `HIServices/AXUIElement.h` |
| `kAXSelectedTextAttribute` **not writable**; `kAXSelectedTextRangeAttribute` writable | `AXAttributeConstants.h` doc comments ("Writable? No." / "Writable? Yes.") |
| `AXStringForRange`, `AXRangeForLine`, `AXBoundsForRange`, `AXAttributedStringForRange`, `AXUIElementsForSearchPredicate` | `AXAttributeConstants.h` lines 1307–1324; `NSAccessibilityConstants.h` line 636 (macOS 26.0) |
| `AXUIElementSetMessagingTimeout` global vs per-object semantics | `AXUIElement.h` discussion block |
| Untrusted AX returns **-25204**, not -25211 | Compiled + ran a Swift binary on macOS 26.6.2 (`AXIsProcessTrusted: false`, focused err `-25204`) |
| Chromium ignores AX `AXValue` writes; supports selection + text-marker setters; supports `AXStringForRange`/`AXRangeForLine`/`AXBoundsForRange` | `chromium/src/ui/accessibility/platform/browser_accessibility_cocoa.mm` (`accessibilityIsAttributeSettable:`, `accessibilitySetValue:forAttribute:`, `accessibilityStringForRange:`) |
| `CGEventKeyboardSetUnicodeString` may be ignored by frameworks | `CoreGraphics/CGEvent.h` header comment (quoted verbatim) |
| Window levels (3 / 8 / 25 / 101 / 1500) | `CoreGraphics/CGWindowLevel.h` |
| `.nonactivatingPanel`, `becomesKeyOnlyIfNeeded` | `NSWindow.h` / `NSPanel.h`; Apple docs abstract |
| Collection behaviors incl. macOS 13 Stage Manager trio | Apple docs JSON for each case (`primary`, `auxiliary`, `canJoinAllApplications` introduced 13.0; `canJoinAllSpaces`/`moveToActiveSpace` 10.5; `fullScreenAuxiliary` 10.7; `transient` 10.6) |
| `SMAppService` is the macOS 13+ login-item API | Apple docs JSON + overview text |
| `LSUIElement` semantics | Apple Information Property List docs JSON |
| `SCStream` since macOS 12.3 | Apple docs JSON |
| Safari web extensions are app extensions, distributed via MAS **or** Developer ID; `safari-web-extension-converter`; macOS 15 web apps | Apple docs JSON abstract |
| Google Docs moved to canvas rendering May 2021, breaks DOM-based extensions | Google Workspace Updates blog post (quoted) |
| macOS 26 adds `NSAccessibilityFontBold/Italic`, `AXBlockQuoteLevel`, `AXUIElementsForSearchPredicate` | `AppKit/NSAccessibilityConstants.h` lines 97, 151–152, 636 — all `API_AVAILABLE(macos(26.0))` |
| macOS 26 adds `NSGlassEffectView` / `NSGlassEffectContainerView` | `AppKit/NSGlassEffectView.h` (`API_AVAILABLE(macos(26.0))`, `NSGlassEffectViewStyleRegular/Clear`) |
| Writing Tools uses Accessibility + sometimes Screen Recording | Project README, lines 105, 133–136, 152 |
| Sparkle EdDSA / appcast / delta | sparkle-project.org documentation |

## Appendix B — Sources

**Apple**
* [AXUIElement](https://developer.apple.com/documentation/applicationservices/axuielement) · [AXUIElement.h reference](https://developer.apple.com/documentation/applicationservices/axuielement_h)
* [kAXStringForRangeParameterizedAttribute](https://developer.apple.com/documentation/applicationservices/kaxstringforrangeparameterizedattribute) · [AXTextMarkerRange](https://developer.apple.com/documentation/applicationservices/axtextmarkerrange)
* [NSWindow.StyleMask.nonactivatingPanel](https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel) · [NSWindow.CollectionBehavior](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct)
* [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice) · [LSUIElement](https://developer.apple.com/documentation/bundleresources/information-property-list/lsuielement)
* [SCStream](https://developer.apple.com/documentation/screencapturekit/scstream) · [Safari web extensions](https://developer.apple.com/documentation/safariservices/safari-web-extensions)
* [Notarizing macOS software before distribution](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
* Local SDK: `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` (macOS 26.6.2)

**Browser**
* [Chromium `browser_accessibility_cocoa.mm`](https://source.chromium.org/chromium/chromium/src/+/main:ui/accessibility/platform/browser_accessibility_cocoa.mm)
* [Chrome native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
* [Google Workspace: Docs canvas rendering](https://workspaceupdates.googleblog.com/2021/05/Google-Docs-Canvas-Based-Rendering-Update.html)

**Open source / vendors**
* [Writing Tools](https://github.com/theJayTea/WritingTools) · [Hammerspoon `hs.axuielement`](https://www.hammerspoon.org/docs/hs.axuielement.html) · [AXSwift](https://github.com/tmandry/AXSwift) · [Swindler](https://github.com/tmandry/Swindler)
* [Ice](https://github.com/jordanbaird/Ice) · [Rectangle](https://github.com/rxhanson/Rectangle) · [Espanso](https://github.com/espanso/espanso) · [Maccy](https://github.com/p0deje/Maccy) · [PopClip](https://www.popclip.app/)
* [Sparkle documentation](https://sparkle-project.org/documentation/)
