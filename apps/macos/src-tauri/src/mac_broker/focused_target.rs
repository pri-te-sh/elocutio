use accessibility::ui_element::AXUIElement;
use accessibility_sys::pid_t;

/// The last externally focused application and its exact focused accessibility element.
///
/// The owned AX reference keeps the element handle alive while the overlay has focus, but does not
/// prevent the source application from removing the element. A failed focus lookup leaves no element
/// rather than retaining an older field or remembering an application-wide traversal fallback.
#[derive(Clone)]
pub struct FocusedTarget {
    pub pid: pid_t,
    pub element: Option<AXUIElement>,
}

impl FocusedTarget {
    /// Returns the focused editable text element if one is active.
    pub fn traversal_root(&self) -> Option<AXUIElement> {
        self.element.clone()
    }
}
