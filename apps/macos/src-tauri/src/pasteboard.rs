//! Transient, concealed NSPasteboard writes with text restore.
//!
//! Rewrite Selection must not pollute clipboard history or destroy the user's
//! clipboard. We snapshot the current plain text, write the rewrite with the
//! `org.nspasteboard.TransientType` and `…ConcealedType` markers, then restore
//! the text if no one else changed the pasteboard meanwhile.
//!
//! Rich pasteboard items are deliberately not restored yet; item-level restore
//! needs a safer reconstruction path than writing back stale NSPasteboardItem
//! objects.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSArray, NSInteger, NSString};

pub struct PasteboardSnapshot {
    original_text: Option<String>,
    own_change_count: NSInteger,
}

/// Writes `text` as a transient, concealed string and returns a restore token.
pub fn write_transient(text: &str) -> Result<PasteboardSnapshot, String> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let text = NSString::from_str(text);
    let transient = NSString::from_str("org.nspasteboard.TransientType");
    let concealed = NSString::from_str("org.nspasteboard.ConcealedType");
    let empty = NSString::from_str("");
    let string_type = unsafe { NSPasteboardTypeString };
    let original_text = pasteboard
        .stringForType(string_type)
        .map(|value| value.to_string());
    let types = NSArray::from_slice(&[string_type, &transient, &concealed]);

    pasteboard.clearContents();
    // SAFETY: the owner is optional and unused by our write.
    unsafe {
        pasteboard.declareTypes_owner(&types, None);
    }

    if !pasteboard.setString_forType(&text, string_type) {
        return Err("Could not write the rewrite to the pasteboard.".into());
    }
    pasteboard.setString_forType(&empty, &transient);
    pasteboard.setString_forType(&empty, &concealed);

    Ok(PasteboardSnapshot {
        original_text,
        own_change_count: pasteboard.changeCount(),
    })
}

/// Restores the snapshot if the pasteboard has not changed since our write.
pub fn restore(snapshot: PasteboardSnapshot) {
    let pasteboard = NSPasteboard::generalPasteboard();
    if pasteboard.changeCount() != snapshot.own_change_count {
        // Someone else changed the clipboard; leave their content alone.
        return;
    }

    pasteboard.clearContents();
    if let Some(original_text) = snapshot.original_text {
        let text = NSString::from_str(&original_text);
        pasteboard.setString_forType(&text, unsafe { NSPasteboardTypeString });
    }
}
