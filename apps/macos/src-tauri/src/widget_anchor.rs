//! Where the floating widget sits, per app.
//!
//! The plan asks for a widget that "draggable to anchor points; position saved
//! per app/site". The default is beside the first underline, or in the screen
//! corner when there is nothing to underline. The user can pin it to a
//! corner instead, and that choice is remembered per app.
//!
//! The position is stored as an anchor rather than as pixels, so it survives a
//! change of display resolution or a different screen.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A corner of the screen the widget can be pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl AnchorCorner {
    /// Every corner, for the settings UI.
    pub const ALL: [AnchorCorner; 4] = [
        AnchorCorner::TopLeft,
        AnchorCorner::TopRight,
        AnchorCorner::BottomLeft,
        AnchorCorner::BottomRight,
    ];

    /// A short label for the settings UI.
    pub const fn label(self) -> &'static str {
        match self {
            AnchorCorner::TopLeft => "Top left",
            AnchorCorner::TopRight => "Top right",
            AnchorCorner::BottomLeft => "Bottom left",
            AnchorCorner::BottomRight => "Bottom right",
        }
    }
}

/// Where the widget is anchored for one app.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WidgetAnchor {
    pub corner: AnchorCorner,
    /// How far from the screen edge, in points.
    pub offset: f32,
}

impl Default for WidgetAnchor {
    fn default() -> Self {
        Self {
            corner: AnchorCorner::BottomRight,
            offset: 24.0,
        }
    }
}

impl WidgetAnchor {
    /// The position of the widget's top-left corner, in screen coordinates.
    pub fn position(&self, screen: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
        let left = match self.corner {
            AnchorCorner::TopLeft | AnchorCorner::BottomLeft => screen.left() + self.offset,
            AnchorCorner::TopRight | AnchorCorner::BottomRight => {
                screen.right() - size.x - self.offset
            }
        };
        let top = match self.corner {
            AnchorCorner::TopLeft | AnchorCorner::TopRight => screen.top() + self.offset,
            AnchorCorner::BottomLeft | AnchorCorner::BottomRight => {
                screen.bottom() - size.y - self.offset
            }
        };
        egui::pos2(left, top)
    }
}

/// The widget anchors for every app the user has pinned one for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WidgetAnchors {
    by_app: BTreeMap<String, WidgetAnchor>,
}

impl WidgetAnchors {
    /// The anchor for an app, if the user pinned one.
    pub fn get(&self, bundle_id: &str) -> Option<WidgetAnchor> {
        self.by_app.get(bundle_id).copied()
    }

    /// Pin an app to a corner.
    pub fn set(&mut self, bundle_id: &str, anchor: WidgetAnchor) {
        self.by_app.insert(bundle_id.to_string(), anchor);
    }

    /// Unpin an app, so it returns to the default position.
    pub fn clear(&mut self, bundle_id: &str) {
        self.by_app.remove(bundle_id);
    }

    /// How many apps are pinned.
    pub fn len(&self) -> usize {
        self.by_app.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_app.is_empty()
    }

    /// Every pinned app and its anchor, for the settings UI.
    pub fn entries(&self) -> Vec<(String, WidgetAnchor)> {
        self.by_app
            .iter()
            .map(|(bundle_id, anchor)| (bundle_id.clone(), *anchor))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 800.0))
    }

    #[test]
    fn the_bottom_right_corner_is_anchored_to_the_bottom_right() {
        let anchor = WidgetAnchor {
            corner: AnchorCorner::BottomRight,
            offset: 24.0,
        };
        let position = anchor.position(screen(), egui::vec2(148.0, 36.0));
        assert_eq!(position.x, 1000.0 - 148.0 - 24.0);
        assert_eq!(position.y, 800.0 - 36.0 - 24.0);
    }

    #[test]
    fn the_top_left_corner_is_anchored_to_the_top_left() {
        let anchor = WidgetAnchor {
            corner: AnchorCorner::TopLeft,
            offset: 10.0,
        };
        let position = anchor.position(screen(), egui::vec2(148.0, 36.0));
        assert_eq!(position.x, 10.0);
        assert_eq!(position.y, 10.0);
    }

    #[test]
    fn an_unpinned_app_has_no_anchor() {
        let anchors = WidgetAnchors::default();
        assert!(anchors.get("com.apple.TextEdit").is_none());
    }

    #[test]
    fn a_pinned_app_keeps_its_anchor() {
        let mut anchors = WidgetAnchors::default();
        let anchor = WidgetAnchor {
            corner: AnchorCorner::TopLeft,
            offset: 12.0,
        };
        anchors.set("com.apple.TextEdit", anchor);
        assert_eq!(anchors.get("com.apple.TextEdit"), Some(anchor));
        assert_eq!(anchors.len(), 1);
    }

    #[test]
    fn clearing_an_anchor_restores_the_default() {
        let mut anchors = WidgetAnchors::default();
        anchors.set(
            "com.apple.TextEdit",
            WidgetAnchor {
                corner: AnchorCorner::TopLeft,
                offset: 12.0,
            },
        );
        anchors.clear("com.apple.TextEdit");
        assert!(anchors.get("com.apple.TextEdit").is_none());
    }

    #[test]
    fn the_anchors_round_trip_through_json() {
        let mut anchors = WidgetAnchors::default();
        anchors.set(
            "com.apple.TextEdit",
            WidgetAnchor {
                corner: AnchorCorner::BottomLeft,
                offset: 8.0,
            },
        );
        let json = serde_json::to_string(&anchors).unwrap();
        let decoded: WidgetAnchors = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, anchors);
    }
}
