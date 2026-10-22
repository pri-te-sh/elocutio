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

pub fn frontmost_window_frame_for_pid(pid: pid_t) -> Option<Rect> {
    window_frame_for_pid(pid, None)
}

pub fn window_frame_for_pid(pid: pid_t, matching_rect: Option<Rect>) -> Option<Rect> {
    let window_infos = core_graphics::window::copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let window_infos =
        unsafe { CFArray::<WindowInfo>::wrap_under_get_rule(window_infos.as_concrete_TypeRef()) };

    let candidates: Vec<Rect> = window_infos
        .iter()
        .filter(|window_info| window_pid(window_info) == Some(pid))
        .filter(|window_info| window_layer(window_info) == Some(0))
        .filter(|window_info| window_alpha(window_info).is_some_and(|alpha| alpha > 0.05))
        .filter_map(|window_info| window_bounds(&window_info))
        .filter(|bounds| bounds.width >= 80.0 && bounds.height >= 50.0)
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
pub fn occluding_window_rects_above_pid(target_pid: pid_t) -> Vec<Rect> {
    let Some(window_infos) = core_graphics::window::copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    let window_infos =
        unsafe { CFArray::<WindowInfo>::wrap_under_get_rule(window_infos.as_concrete_TypeRef()) };

    let mut occluders = Vec::new();
    for window_info in window_infos.iter() {
        // Only normal application windows (layer == 0) can occlude another normal window.
        // Higher layers (Dock at layer 20, WindowServer at layer 24, elo-overlay at layer 25)
        // are system/overlay surfaces and must never be treated as window occluders.
        if window_layer(&window_info) != Some(0) {
            continue;
        }
        if !window_alpha(&window_info).is_some_and(|alpha| alpha > 0.05) {
            continue;
        }
        let Some(bounds) = window_bounds(&window_info) else {
            continue;
        };
        if bounds.width < 80.0 || bounds.height < 50.0 {
            continue;
        }
        let Some(pid) = window_pid(&window_info) else {
            continue;
        };
        // Stop once we reach the target application's frontmost standard window
        if pid == target_pid {
            break;
        }
        occluders.push(bounds);
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
