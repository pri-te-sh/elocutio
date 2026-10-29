use accessibility_sys::pid_t;
use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_graphics::window::{
    kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
    kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID,
};
use std::time::{Duration, Instant};

use crate::rect::Rect;

use super::core_foundation_utilities::{
    WindowInfo, dictionary_dictionary, dictionary_f64, dictionary_i64,
};

pub const WINDOW_MOVEMENT_SETTLE_DURATION: Duration = Duration::from_millis(150);
const WINDOW_FRAME_TOLERANCE: f64 = 0.5;

/// Last observed frontmost window frame for a target process.
pub struct WindowMovementState {
    pub pid: pid_t,
    pub frame: Rect,
    pub last_changed_at: Instant,
}

pub fn window_frame_changed(previous: Rect, current: Rect) -> bool {
    !nearly_equal(previous.x, current.x)
        || !nearly_equal(previous.y, current.y)
        || !nearly_equal(previous.width, current.width)
        || !nearly_equal(previous.height, current.height)
}

pub fn settled_window_state(pid: pid_t, frame: Rect, now: Instant) -> WindowMovementState {
    WindowMovementState {
        pid,
        frame,
        last_changed_at: now
            .checked_sub(WINDOW_MOVEMENT_SETTLE_DURATION)
            .unwrap_or(now),
    }
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= WINDOW_FRAME_TOLERANCE
}

/// One on-screen window, parsed from `CGWindowListCopyWindowInfo`.
///
/// The window list is expensive to copy, so a poll takes it once with [`on_screen_windows`] and
/// hands the same slice to every query below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnScreenWindow {
    pub pid: pid_t,
    pub layer: i64,
    pub alpha: f64,
    pub bounds: Rect,
}

impl OnScreenWindow {
    /// A visible, normal-level window big enough to be a real document window.
    fn is_standard_visible(&self) -> bool {
        self.layer == 0
            && self.alpha > 0.05
            && self.bounds.width >= 80.0
            && self.bounds.height >= 50.0
    }
}

/// Every on-screen window, front to back. Entries missing a field the queries need are skipped.
pub fn on_screen_windows() -> Vec<OnScreenWindow> {
    let Some(window_infos) = core_graphics::window::copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    let window_infos =
        unsafe { CFArray::<WindowInfo>::wrap_under_get_rule(window_infos.as_concrete_TypeRef()) };

    window_infos
        .iter()
        .filter_map(|window_info| {
            Some(OnScreenWindow {
                pid: window_pid(&window_info)?,
                layer: window_layer(&window_info)?,
                alpha: window_alpha(&window_info)?,
                bounds: window_bounds(&window_info)?,
            })
        })
        .collect()
}

pub fn frontmost_window_frame_for_pid(windows: &[OnScreenWindow], pid: pid_t) -> Option<Rect> {
    window_frame_for_pid(windows, pid, None)
}

pub fn window_frame_for_pid(
    windows: &[OnScreenWindow],
    pid: pid_t,
    matching_rect: Option<Rect>,
) -> Option<Rect> {
    let candidates: Vec<Rect> = windows
        .iter()
        .filter(|window| window.pid == pid && window.is_standard_visible())
        .map(|window| window.bounds)
        .collect();

    if let Some(target) = matching_rect
        && let Some(matched) = candidates.iter().copied().find(|b| b.intersects(target))
    {
        return Some(matched);
    }

    candidates
        .iter()
        .copied()
        .find(|b| b.width >= 160.0 && b.height >= 140.0)
        .or_else(|| candidates.into_iter().next())
}

/// Collects screen-space rectangles of any on-screen windows that sit in front of
/// `target_pid`'s frontmost window in Z-order (excluding the `elo-overlay` window at layer 25
/// and the macOS menu bar).
pub fn occluding_window_rects_above_pid(
    windows: &[OnScreenWindow],
    target_pid: pid_t,
) -> Vec<Rect> {
    let mut occluders = Vec::new();
    // Only normal application windows (layer == 0) can occlude another normal window.
    // Higher layers (Dock at layer 20, WindowServer at layer 24, elo-overlay at layer 25)
    // are system/overlay surfaces and must never be treated as window occluders.
    for window in windows.iter().filter(|window| window.is_standard_visible()) {
        // Stop once we reach the target application's frontmost standard window
        if window.pid == target_pid {
            break;
        }
        occluders.push(window.bounds);
    }

    occluders
}

fn window_pid(window_info: &WindowInfo) -> Option<pid_t> {
    dictionary_i64(window_info, unsafe { kCGWindowOwnerPID }).map(|value| value as pid_t)
}

fn window_layer(window_info: &WindowInfo) -> Option<i64> {
    dictionary_i64(window_info, unsafe { kCGWindowLayer })
}

fn window_alpha(window_info: &WindowInfo) -> Option<f64> {
    dictionary_f64(window_info, unsafe { kCGWindowAlpha })
}

fn window_bounds(window_info: &WindowInfo) -> Option<Rect> {
    let bounds = dictionary_dictionary(window_info, unsafe { kCGWindowBounds })?;

    Some(Rect {
        x: dictionary_f64(
            &bounds,
            CFString::from_static_string("X").as_concrete_TypeRef(),
        )?,
        y: dictionary_f64(
            &bounds,
            CFString::from_static_string("Y").as_concrete_TypeRef(),
        )?,
        width: dictionary_f64(
            &bounds,
            CFString::from_static_string("Width").as_concrete_TypeRef(),
        )?,
        height: dictionary_f64(
            &bounds,
            CFString::from_static_string("Height").as_concrete_TypeRef(),
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pid: pid_t, layer: i64, x: f64, width: f64) -> OnScreenWindow {
        OnScreenWindow {
            pid,
            layer,
            alpha: 1.0,
            bounds: Rect::new(x, 0.0, width, 400.0),
        }
    }

    #[test]
    fn occluders_stop_at_the_target_window() {
        let windows = [
            window(1, 25, 0.0, 2000.0), // the overlay itself, never an occluder
            window(2, 0, 10.0, 300.0),
            window(3, 0, 20.0, 300.0),
            window(4, 0, 30.0, 300.0),
        ];
        let occluders = occluding_window_rects_above_pid(&windows, 3);
        assert_eq!(occluders, vec![Rect::new(10.0, 0.0, 300.0, 400.0)]);
    }

    #[test]
    fn the_frame_prefers_the_window_holding_the_field() {
        let windows = [window(7, 0, 0.0, 300.0), window(7, 0, 500.0, 300.0)];
        let field = Rect::new(600.0, 10.0, 50.0, 20.0);
        assert_eq!(
            window_frame_for_pid(&windows, 7, Some(field)),
            Some(Rect::new(500.0, 0.0, 300.0, 400.0))
        );
        assert_eq!(
            frontmost_window_frame_for_pid(&windows, 7),
            Some(Rect::new(0.0, 0.0, 300.0, 400.0))
        );
        assert_eq!(frontmost_window_frame_for_pid(&windows, 8), None);
    }
}
