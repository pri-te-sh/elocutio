use accessibility::ui_element::AXUIElement;
use accessibility_sys::{
    AXUIElementCopyAttributeValue, AXUIElementGetTypeID, AXUIElementSetMessagingTimeout,
    error_string, kAXErrorSuccess, pid_t,
};
use core_foundation::base::{CFGetTypeID, CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::{error::Error as StdError, ptr};

pub type WindowInfo = CFDictionary<CFString, CFType>;

/// How long a single Accessibility call may wait on the target app before failing.
///
/// The default is about six seconds. A beachballing target app would otherwise stall the overlay
/// thread for that long on every call, so a call that cannot answer quickly is treated as failed.
const AX_MESSAGING_TIMEOUT_SECONDS: f32 = 0.25;

/// Applies [`AX_MESSAGING_TIMEOUT_SECONDS`] process-wide.
///
/// Setting the timeout on the system-wide element sets the default for every element this process
/// talks to, including elements obtained from attributes. It only needs to happen once.
pub fn install_global_messaging_timeout() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let system = AXUIElement::system_wide();
        set_messaging_timeout(&system);
    });
}

/// The system-wide accessibility element, with the short messaging timeout in force.
pub fn system_wide_element() -> AXUIElement {
    install_global_messaging_timeout();
    AXUIElement::system_wide()
}

/// The accessibility element for an application, with the short messaging timeout set on it.
pub fn application_element(pid: pid_t) -> AXUIElement {
    install_global_messaging_timeout();
    let app = AXUIElement::application(pid);
    set_messaging_timeout(&app);
    app
}

fn set_messaging_timeout(element: &AXUIElement) {
    let error = unsafe {
        AXUIElementSetMessagingTimeout(element.as_concrete_TypeRef(), AX_MESSAGING_TIMEOUT_SECONDS)
    };
    if error != kAXErrorSuccess {
        eprintln!(
            "Could not set the accessibility messaging timeout: {}",
            error_string(error)
        );
    }
}

pub fn dictionary_i64(dictionary: &WindowInfo, key: CFStringRef) -> Option<i64> {
    dictionary_number(dictionary, key)?.to_i64()
}

pub fn dictionary_f64(dictionary: &WindowInfo, key: CFStringRef) -> Option<f64> {
    dictionary_number(dictionary, key)?.to_f64()
}

fn dictionary_number(dictionary: &WindowInfo, key: CFStringRef) -> Option<CFNumber> {
    let value = dictionary_value(dictionary, key)?;
    // Wrapping a value as the wrong CF type is undefined behaviour, so check the type first.
    if !value.instance_of::<CFNumber>() {
        return None;
    }

    Some(unsafe { CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as _) })
}

pub fn dictionary_dictionary(dictionary: &WindowInfo, key: CFStringRef) -> Option<WindowInfo> {
    let value = dictionary_value(dictionary, key)?;
    if !value.instance_of::<WindowInfo>() {
        return None;
    }

    Some(unsafe { WindowInfo::wrap_under_get_rule(value.as_CFTypeRef() as _) })
}

fn dictionary_value(dictionary: &WindowInfo, key: CFStringRef) -> Option<CFType> {
    let key = unsafe { CFString::wrap_under_get_rule(key) };

    dictionary.find(&key).map(|value| value.clone())
}

/// Grabs a string attribute from an accessibility element.
pub fn ax_string_attribute(element: &AXUIElement, name: &str) -> Result<String, Box<dyn StdError>> {
    let attr = CFString::new(name);
    let mut value = ptr::null();

    let err = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            &mut value,
        )
    };

    if err != kAXErrorSuccess {
        return Err(format!(
            "AXUIElementCopyAttributeValue failed for {name}: {}",
            error_string(err)
        )
        .into());
    }

    if value.is_null() {
        return Err(format!("AXUIElementCopyAttributeValue returned null for {name}").into());
    }

    let value = unsafe { CFType::wrap_under_create_rule(value) };
    if !value.instance_of::<CFString>() {
        return Err(format!("AX attribute {name} is not a string").into());
    }

    let value = unsafe { CFString::wrap_under_get_rule(value.as_CFTypeRef() as _) };
    Ok(value.to_string())
}

/// Grabs the value of an attribute on an accessibility element.
pub fn ax_element_attribute(
    element: &AXUIElement,
    name: &str,
) -> Result<AXUIElement, Box<dyn StdError>> {
    let attr = CFString::new(name);
    let mut value = ptr::null();

    let err = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_concrete_TypeRef(),
            attr.as_concrete_TypeRef(),
            &mut value,
        )
    };

    if err != kAXErrorSuccess {
        return Err(format!(
            "AXUIElementCopyAttributeValue failed: {}",
            error_string(err)
        )
        .into());
    }

    if value.is_null() {
        return Err(format!("AXUIElementCopyAttributeValue returned null for {name}").into());
    }

    // Take ownership first so the value is released on every path, then check that it really is
    // an element before treating it as one.
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    if unsafe { CFGetTypeID(value.as_CFTypeRef()) != AXUIElementGetTypeID() } {
        return Err(format!("AX attribute {name} is not an element").into());
    }

    Ok(unsafe { AXUIElement::wrap_under_get_rule(value.as_CFTypeRef() as _) })
}
