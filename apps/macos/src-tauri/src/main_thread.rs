//! Run AppKit/Accessibility work on the Tauri main thread.

use tauri::{AppHandle, Runtime};

/// Schedules `f` on the main thread and awaits its result without blocking the calling task.
pub async fn run_async<R: Runtime, T, F>(app: &AppHandle<R>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle<R>) -> T + Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let app_for_thread = app.clone();
    app.run_on_main_thread(move || {
        let _ = sender.send(f(&app_for_thread));
    })
    .map_err(|error| format!("Could not schedule main-thread work: {error}"))?;

    receiver
        .await
        .map_err(|error| format!("Main-thread work did not return: {error}"))
}

/// Synchronous form of [`run_async`] for callers that cannot await.
///
/// Waiting on a tokio worker thread would stall every other task scheduled on that worker, so
/// inside the multi-threaded runtime the wait is wrapped in `block_in_place`, which hands the
/// worker's queue to another thread first. Called on the main thread itself, `f` runs inline:
/// scheduling it and then waiting would deadlock.
///
/// Async callers should prefer `run_async(..).await`.
pub fn run<R: Runtime, T, F>(app: &AppHandle<R>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle<R>) -> T + Send + 'static,
{
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_some() {
        return Ok(f(app));
    }

    let work = run_async(app, f);
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(work))
        }
        // A current-thread runtime cannot be blocked from inside, so wait on a helper thread.
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(|| tauri::async_runtime::block_on(work))
                .join()
                .unwrap_or_else(|_| Err("Main-thread work panicked.".to_string()))
        }),
        Err(_) => tauri::async_runtime::block_on(work),
    }
}
