use std::time::Duration;

use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tokio::time::sleep;
use tracing::error;

use tauri::menu::{Menu, MenuBuilder};
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::runtime::Runtime as AsyncRuntime;

use crate::highlighter_service::HighlighterService;
use crate::windows::{show_editor_window, show_settings_window};

/// The label the menu shows for the current suggestion count.
///
/// The plan's menu is "3 suggestions", so the count is part of the menu rather
/// than something the user has to infer from the overlay.
#[allow(dead_code)]
pub fn suggestions_label(count: usize) -> String {
    match count {
        0 => "No suggestions".to_string(),
        1 => "1 suggestion".to_string(),
        count => format!("{count} suggestions"),
    }
}

/// Defines the layout of the tray menu.
fn tray_menu<R: Runtime, M: Manager<R>>(manager: &M, is_running: bool) -> tauri::Result<Menu<R>> {
    let toggle_label = if is_running {
        "Pause Overlay"
    } else {
        "Resume Overlay"
    };

    MenuBuilder::new(manager)
        .text("open-editor", "Open Elocutio Studio")
        .text("toggle-service", toggle_label)
        .separator()
        .text("settings", "Settings…")
        .separator()
        .text("quit", "Quit Elocutio")
        .build()
}

pub fn set_up_tray_menu(app: &AppHandle) -> tauri::Result<()> {
    let highlighter_service: State<HighlighterService> = app.state();
    let async_runtime: State<AsyncRuntime> = app.state();

    let initial_is_running = highlighter_service.is_running();

    let tray_icon = TrayIconBuilder::new()
        .icon(menu_bar_icon(initial_is_running)?)
        .icon_as_template(true)
        .menu(&tray_menu(app, initial_is_running)?)
        .on_menu_event(move |app, event| {
            let event_id = event.id().0.as_str();

            match event_id {
                "toggle-service" => {
                    let service: State<HighlighterService> = app.state();

                    let _ = service
                        .toggle()
                        .inspect_err(|err| error!("Could not toggle highlighter: {err}"));
                }
                "open-editor" => {
                    crate::elo_overlay::set_studio_launch_state(Some("editor".to_string()), false);
                    let _ = show_editor_window(app)
                        .inspect_err(|err| error!("Could not open the editor window: {err}"));
                }
                "settings" => {
                    crate::elo_overlay::set_studio_launch_state(Some("apps".to_string()), false);
                    let _ = show_settings_window(app)
                        .inspect_err(|err| error!("Could not open the settings window: {err}"));
                }
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

            if let Ok(updated_menu) = tray_menu(&app, is_running) {
                let _ = tray_icon.set_menu(Some(updated_menu));
            }

            match tray_icon.set_icon(Some(new_icon)) {
                Ok(()) => {
                    let _ = tray_icon.set_icon_as_template(true);
                    shown_is_running = is_running;
                }
                // Leave the remembered state alone so the next tick retries.
                Err(err) => error!("Unable to set new icon: {err}"),
            }
        }
    });

    Ok(())
}

pub fn menu_bar_icon(is_running: bool) -> tauri::Result<Image<'static>> {
    let icon = Image::from_bytes(include_bytes!("../icons/menu-bar-icon.png"))?;
    let width = icon.width();
    let height = icon.height();
    let mut rgba = icon.rgba().to_vec();

    if !is_running {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = ((pixel[3] as u16 * 45) / 100) as u8;
        }
    }

    draw_status_line(&mut rgba, width, height, is_running);

    Ok(Image::new_owned(rgba, width, height))
}

fn draw_status_line(rgba: &mut [u8], width: u32, height: u32, is_running: bool) {
    let line_height = (height.min(width) / 14).max(3);
    let start_y = height.saturating_sub(line_height);
    let margin_x = width / 8;

    for y in start_y..height {
        for x in margin_x..width.saturating_sub(margin_x) {
            if !is_running && ((x / 4) % 2 == 1) {
                continue;
            }
            let alpha = if is_running { 235u8 } else { 110u8 };
            let index = ((y * width + x) * 4) as usize;
            rgba[index..index + 4].copy_from_slice(&[0, 0, 0, alpha]);
        }
    }
}
