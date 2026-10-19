//! Run AppKit/Accessibility work on the Tauri main thread.

use tauri::AppHandle;

pub fn run<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> T + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();
    let app_for_thread = app.clone();
    app.run_on_main_thread(move || {
        let _ = sender.send(f(&app_for_thread));
    })
    .map_err(|error| format!("Could not schedule main-thread work: {error}"))?;

    receiver
        .recv()
        .map_err(|error| format!("Main-thread work did not return: {error}"))
}
