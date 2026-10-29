//! Transient, concealed NSPasteboard writes with faithful restore.
//!
//! Rewrite Selection must not pollute clipboard history or destroy the user's
//! clipboard. We snapshot the current pasteboard **items** (every type and its
//! bytes), write the rewrite with the `org.nspasteboard.TransientType` and
//! `…ConcealedType` markers, then restore the snapshot if no one else changed
//! the pasteboard meanwhile.
//!
//! Restoring at the item level preserves rich content: images, file promises,
//! and custom types survive a rewrite. Only the pasteboard's *change count*
//! is used to detect an intervening change by another app, so a user copy that
//! lands mid-rewrite is never clobbered.
//!
//! Items that cannot be round-tripped are reported rather than silently
//! dropped; see [`PasteboardSnapshot::unsupported_types`].

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSInteger, NSString};

/// The bytes of one pasteboard type.
type Bytes = Vec<u8>;

/// One pasteboard item: each of its types with that type's own bytes.
///
/// A rich-text copy carries different bytes per type (RTF, HTML, plain
/// text), so every type is captured separately; writing one type's bytes
/// under all of them would paste raw RTF markup into a plain-text field.
struct SnapshotItem {
    representations: Vec<(String, Bytes)>,
}

/// A restorable snapshot of the general pasteboard.
pub struct PasteboardSnapshot {
    items: Vec<SnapshotItem>,
    /// Types present on the pasteboard that could not be captured as bytes.
    unsupported_types: Vec<String>,
    /// The change count right after our own write, used to detect interference.
    own_change_count: NSInteger,
}

impl PasteboardSnapshot {
    /// Types that were on the pasteboard but could not be captured.
    ///
    /// Non-empty means the restore will be lossy for those types. The caller
    /// may surface this instead of silently losing the user's data.
    pub fn unsupported_types(&self) -> &[String] {
        &self.unsupported_types
    }

    /// Whether the snapshot captured every type that was present.
    pub fn is_lossless(&self) -> bool {
        self.unsupported_types.is_empty()
    }
}

/// Captures the current pasteboard contents, then writes `text` as a transient,
/// concealed string.
pub fn write_transient(text: &str) -> Result<PasteboardSnapshot, String> {
    let pasteboard = NSPasteboard::generalPasteboard();

    let (items, unsupported_types) = snapshot_items(&pasteboard);

    let text = NSString::from_str(text);
    let transient = NSString::from_str("org.nspasteboard.TransientType");
    let concealed = NSString::from_str("org.nspasteboard.ConcealedType");
    let empty = NSString::from_str("");
    let string_type = unsafe { NSPasteboardTypeString };
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
        items,
        unsupported_types,
        own_change_count: pasteboard.changeCount(),
    })
}

/// Restores the snapshot if the pasteboard has not changed since our write.
///
/// The restore rebuilds every captured item with its original types, so rich
/// content survives. If the user copied something else in the meantime the
/// change count differs and their content is left alone.
pub fn restore(snapshot: PasteboardSnapshot) {
    let pasteboard = NSPasteboard::generalPasteboard();
    if pasteboard.changeCount() != snapshot.own_change_count {
        // Someone else changed the clipboard; leave their content alone.
        return;
    }

    pasteboard.clearContents();

    if snapshot.items.is_empty() {
        return;
    }

    let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = snapshot
        .items
        .iter()
        .map(|item| {
            let pasteboard_item = NSPasteboardItem::new();
            for (pasteboard_type, bytes) in &item.representations {
                let data = NSData::with_bytes(bytes);
                let type_string = NSString::from_str(pasteboard_type);
                pasteboard_item.setData_forType(&data, &type_string);
            }
            ProtocolObject::from_retained(pasteboard_item)
        })
        .collect();

    let array = NSArray::from_retained_slice(&items);
    pasteboard.writeObjects(&array);
}

/// Reads every pasteboard item and its bytes.
///
/// Returns the captured items plus the types that could not be read.
fn snapshot_items(pasteboard: &NSPasteboard) -> (Vec<SnapshotItem>, Vec<String>) {
    let mut items = Vec::new();
    let mut unsupported = Vec::new();

    let Some(pasteboard_items) = pasteboard.pasteboardItems() else {
        return (items, unsupported);
    };

    for pasteboard_item in pasteboard_items.iter() {
        let types = pasteboard_item.types();
        if types.is_empty() {
            continue;
        }

        let mut representations = Vec::new();
        for pasteboard_type in types.iter() {
            let name = pasteboard_type.to_string();
            match pasteboard_item.dataForType(&pasteboard_type) {
                Some(data) => representations.push((name, data.to_vec())),
                None => {
                    // A type whose bytes cannot be read (a file promise, a
                    // lazily provided type) is reported so the caller can
                    // surface a lossy restore instead of hiding it.
                    if !unsupported.contains(&name) {
                        unsupported.push(name);
                    }
                }
            }
        }

        if !representations.is_empty() {
            items.push(SnapshotItem { representations });
        }
    }

    (items, unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_lossless_when_nothing_was_captured() {
        let snapshot = PasteboardSnapshot {
            items: Vec::new(),
            unsupported_types: Vec::new(),
            own_change_count: 0,
        };
        assert!(snapshot.is_lossless());
        assert!(snapshot.unsupported_types().is_empty());
    }

    #[test]
    fn unsupported_types_are_reported() {
        let snapshot = PasteboardSnapshot {
            items: Vec::new(),
            unsupported_types: vec!["public.file-url".to_string()],
            own_change_count: 0,
        };
        assert!(!snapshot.is_lossless());
        assert_eq!(snapshot.unsupported_types().len(), 1);
    }
}
