use std::time::Duration;

use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tokio::time::sleep;
use tracing::error;

use tauri::menu::{Menu, MenuBuilder};
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::runtime::Runtime as AsyncRuntime;

use crate::highlighter_service::HighlighterService;
use crate::windows::{open_issue_report, show_editor_window, show_settings_window};

/// The label the menu shows for the current suggestion count.
///
/// The plan's menu is "3 suggestions", so the count is part of the menu rather
/// than something the user has to infer from the overlay.
pub fn suggestions_label(count: usize) -> String {
    match count {
        0 => "No suggestions".to_string(),
        1 => "1 suggestion".to_string(),
        count => format!("{count} suggestions"),
    }
}

/// Defines the layout of the tray menu.
fn tray_menu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
    let count = manager
        .try_state::<HighlighterService>()
        .map(|state| state.suggestion_count())
        .unwrap_or(0);

    MenuBuilder::new(manager)
        .text("suggestions", suggestions_label(count))
        .separator()
        .text("focus-card", "Show Suggestions")
        .text("next-suggestion", "Next Suggestion")
        .text("previous-suggestion", "Previous Suggestion")
        .text("accept-suggestion", "Accept Suggestion")
        .text("dismiss-suggestion", "Dismiss Suggestion")
        .separator()
        .text("toggle-service", "Toggle Service")
        .text("rewrite-selection", "Rewrite Selection")
        .text("analyze-field", "Analyze Field")
        .text("undo-edit", "Undo Last Edit")
        .text("open-editor", "Open Editor")
        .text("settings", "Settings")
        .text("report-issue", "Report Issue")
        .text("quit", "Quit")
        .build()
}

pub fn set_up_tray_menu(app: &AppHandle) -> tauri::Result<()> {
    let highlighter_service: State<HighlighterService> = app.state();
    let async_runtime: State<AsyncRuntime> = app.state();

    let initial_is_running = highlighter_service.is_running();

    let tray_icon = TrayIconBuilder::new()
        .icon(menu_bar_icon(initial_is_running)?)
        .menu(&tray_menu(app)?)
        .on_menu_event(move |app, event| {
            let event_id = event.id().0.as_str();

            match event_id {
                "focus-card" => request_card(app, crate::config::CardCommand::Focus),
                "next-suggestion" => request_card(app, crate::config::CardCommand::NextSuggestion),
                "previous-suggestion" => {
                    request_card(app, crate::config::CardCommand::PreviousSuggestion)
                }
                "accept-suggestion" => request_card(app, crate::config::CardCommand::Accept),
                "dismiss-suggestion" => request_card(app, crate::config::CardCommand::Dismiss),
                "toggle-service" => {
                    let service: State<HighlighterService> = app.state();

                    let _ = service
                        .toggle()
                        .inspect_err(|err| error!("Could not toggle highlighter: {err}"));
                }
                "rewrite-selection" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match crate::rewrite::rewrite_selection(&app).await {
                            Ok(message) => crate::rewrite::notify(&app, "Elocutio", &message),
                            Err(error) => crate::rewrite::notify(&app, "Rewrite failed", &error),
                        }
                    });
                }
                "analyze-field" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let result = crate::llm_analysis::analyze_focused_field(&app).await;
                        crate::llm_analysis::notify_result(&app, result);
                    });
                }
                "undo-edit" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match crate::undo::undo_last_edit(&app).await {
                            Ok(message) => crate::rewrite::notify(&app, "Elocutio undo", &message),
                            Err(error) => crate::rewrite::notify(&app, "Nothing to undo", &error),
                        }
                    });
                }
                "open-editor" => {
                    let _ = show_editor_window(app)
                        .inspect_err(|err| error!("Could not open the editor window: {err}"));
                }
                "settings" => {
                    let _ = show_settings_window(app)
                        .inspect_err(|err| error!("Could not open the settings window: {err}"));
                }
                "report-issue" => open_issue_report(app),
                "quit" => app.exit(0),
                _ => error!("Encountered unexpected event: `{event_id}`"),
            };
        })
        .build(app)?;

    let app = app.clone();
    async_runtime.spawn(async move {
        let mut shown_is_running = initial_is_running;

        loop {
            sleep(Duration::from_millis(250)).await;

            let is_running = {
                let highlighter_service: State<HighlighterService> = app.state();
                highlighter_service.is_running()
            };

            // Only touch the tray when the service state actually changes.
            // Re-decoding the icon and calling set_icon four times a second
            // regardless is pure waste, and on Windows it eventually starts
            // failing: after several days the shell began rejecting every
            // update with ERROR_TIMEOUT, filling the log and leaving the tray
            // icon unresponsive.
            if is_running == shown_is_running {
                continue;
            }

            let Ok(new_icon) = menu_bar_icon(is_running) else {
                error!("Unable to generate new menu bar icon.");
                continue;
            };

            match tray_icon.set_icon(Some(new_icon)) {
                Ok(()) => shown_is_running = is_running,
                // Leave the remembered state alone so the next tick retries.
                Err(err) => error!("Unable to set new icon: {err}"),
            }
        }
    });

    Ok(())
}

/// Asks the highlighter to run a card command.
///
/// The command travels with the configuration poll, so the tray menu and the
/// keyboard shortcuts drive the card the same way.
fn request_card(app: &AppHandle, command: crate::config::CardCommand) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::config::set_card_command(&app, command).await {
            error!("Could not send the card command: {error}");
        }
    });
}

fn service_status_color(is_running: bool) -> [u8; 4] {
    match is_running {
        true => [34, 197, 94, 255],
        false => [239, 68, 68, 255],
    }
}

pub fn menu_bar_icon(is_running: bool) -> tauri::Result<Image<'static>> {
    let icon = Image::from_bytes(include_bytes!("../icons/menu-bar-icon.png"))?;
    let width = icon.width();
    let height = icon.height();
    let mut rgba = icon.rgba().to_vec();

    draw_status_line(&mut rgba, width, height, service_status_color(is_running));

    Ok(Image::new_owned(rgba, width, height))
}

fn draw_status_line(rgba: &mut [u8], width: u32, height: u32, color: [u8; 4]) {
    let line_height = (height.min(width) / 14).max(3);
    let start_y = height.saturating_sub(line_height);

    for y in start_y..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            rgba[index..index + 4].copy_from_slice(&color);
        }
    }
}
