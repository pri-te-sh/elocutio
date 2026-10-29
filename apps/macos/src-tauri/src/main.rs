// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;
mod undo;
mod voice;
mod windows;

use self::highlighter_service::HighlighterService;
use self::tray::set_up_tray_menu;
use crate::args::Args;
use crate::config::Config;
use clap::Parser;
use serde::Serialize;
use std::io::stderr;
use std::sync::{Arc, RwLock};
use tauri::Manager as _;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tracing::{Level, error};
use tracing_subscriber::FmtSubscriber;

use crate::os_broker::{AccessibilityPermissionStatus, OsBroker};
use tokio::{runtime::Builder, sync::Mutex};

pub mod app_matrix;
mod app_matrix_consistency;
mod args;
mod auto_analyze;
mod coalesce;
pub mod color;
mod commands;
pub mod config;
pub mod elo_overlay;
pub mod events;
pub mod gate;
pub mod highlighter_service;
pub mod lint_kind_color;
mod llm_analysis;
mod llm_cache;
mod main_thread;
pub mod matrix;
pub mod network_log;
mod openrouter;
mod os_broker;
pub mod personalization_db;
pub mod preview;
pub mod quality;
pub mod rect;
mod rewrite;
mod usage;
pub mod widget_anchor;

#[cfg(target_os = "macos")]
pub mod apple_fm_bridge;
#[cfg(target_os = "macos")]
mod mac_broker;
#[cfg(target_os = "macos")]
mod pasteboard;

#[cfg(target_os = "windows")]
mod windows_broker;

#[derive(Debug, Clone, Serialize)]
struct IntegrationView {
    bundle_id: String,
    enabled: bool,
    display_name: String,
}

#[cfg(target_os = "macos")]
pub(crate) type PlatformBroker = mac_broker::MacBroker;

#[cfg(target_os = "windows")]
pub(crate) type PlatformBroker = windows_broker::WindowsBroker;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) type PlatformBroker = os_broker::NoopBroker;

/// Creates the process-local platform broker.
///
/// The Tauri process stores its single broker as managed state, while the highlighter subprocess
/// creates its own broker because it is a separate process.
fn platform_broker(
    is_integration_enabled: impl FnMut(&str) -> bool + Send + Sync + 'static,
) -> PlatformBroker {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        PlatformBroker::new(is_integration_enabled)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = is_integration_enabled;
        PlatformBroker::default()
    }
}

/// Registers the system-wide Rewrite Selection shortcut.
fn register_shortcuts(app: &tauri::AppHandle) {
    let shortcut = Shortcut::new(Some(Modifiers::ALT | Modifiers::SUPER), Code::KeyE);
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }

            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // The first press prepares the rewrite and shows what would
                // change; the second applies exactly what was shown. A rewrite
                // the user has not seen is a rewrite they did not ask for.
                if crate::rewrite::load_pending().ok().flatten().is_some() {
                    match crate::rewrite::apply_pending(&app).await {
                        Ok(message) => crate::rewrite::notify(&app, "Elocutio", &message),
                        Err(error) => crate::rewrite::notify(&app, "Rewrite failed", &error),
                    }
                } else {
                    match crate::rewrite::preview_selection(&app).await {
                        Ok(pending) => crate::rewrite::notify(
                            &app,
                            "Elocutio rewrite ready",
                            &format!(
                                "Press ⌥⌘E again to apply. {} → {} characters.",
                                pending.original.chars().count(),
                                pending.rewritten.chars().count()
                            ),
                        ),
                        Err(error) => crate::rewrite::notify(&app, "Rewrite failed", &error),
                    }
                }
            });
        });

    if let Err(error) = result {
        eprintln!("Could not register the Rewrite Selection shortcut: {error}");
    }

    let analyze_shortcut = Shortcut::new(Some(Modifiers::ALT | Modifiers::SUPER), Code::KeyA);
    let analyze_result =
        app.global_shortcut()
            .on_shortcut(analyze_shortcut, |app, _shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }

                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let result = crate::llm_analysis::analyze_focused_field(&app).await;
                    crate::llm_analysis::notify_result(&app, result);
                });
            });

    if let Err(error) = analyze_result {
        eprintln!("Could not register the Analyze Field shortcut: {error}");
    }

    let undo_shortcut = Shortcut::new(Some(Modifiers::ALT | Modifiers::SUPER), Code::KeyZ);
    let undo_result = app
        .global_shortcut()
        .on_shortcut(undo_shortcut, |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }

            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::undo::undo_last_edit(&app).await {
                    Ok(message) => crate::rewrite::notify(&app, "Elocutio undo", &message),
                    Err(error) => crate::rewrite::notify(&app, "Nothing to undo", &error),
                }
            });
        });

    if let Err(error) = undo_result {
        eprintln!("Could not register the Elocutio Undo shortcut: {error}");
    }

    register_card_shortcuts(app);
}

/// Registers the shortcuts that drive the suggestion card.
///
/// The card lives in the highlighter process and its window never takes
/// keyboard focus — activating it would move the caret and break the paste
/// that follows (PLAN.md §5.7, "Focus is sacred"). So the keyboard model is
/// global hotkeys, and the command travels to the card with the
/// configuration poll.
///
/// The modifiers are the plan's own, so the muscle memory is the same:
/// `⌃⌥G` focuses the card, the arrows navigate, `Return` accepts, and `Esc`
/// closes.
fn register_card_shortcuts(app: &tauri::AppHandle) {
    use crate::config::CardCommand;

    // (shortcut, command, label)
    let bindings: [(Modifiers, Code, CardCommand, &str); 6] = [
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyG,
            CardCommand::Focus,
            "focus the card",
        ),
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyN,
            CardCommand::NextSuggestion,
            "next suggestion",
        ),
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyB,
            CardCommand::PreviousSuggestion,
            "previous suggestion",
        ),
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyA,
            CardCommand::Accept,
            "accept the suggestion",
        ),
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyD,
            CardCommand::Dismiss,
            "dismiss the suggestion",
        ),
        (
            Modifiers::CONTROL | Modifiers::ALT,
            Code::KeyK,
            CardCommand::Close,
            "close the card",
        ),
    ];

    for (modifiers, code, command, label) in bindings {
        let shortcut = Shortcut::new(Some(modifiers), code);
        let result = app
            .global_shortcut()
            .on_shortcut(shortcut, move |app, _shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                request_card_command(app, command);
            });
        if let Err(error) = result {
            eprintln!("Could not register the shortcut to {label}: {error}");
        }
    }
}

/// Asks the overlay to run a card command.
///
/// The command is held in the shared configuration in memory and the overlay
/// loop consumes it on its next poll, which is triggered immediately. It is
/// deliberately not written to disk: a keypress is not a setting.
fn request_card_command(app: &tauri::AppHandle, command: crate::config::CardCommand) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<Mutex<Config>>>();
        let mut config = state.lock().await;
        config.card_command = Some(command);
        drop(config);
        elo_overlay::request_refresh();
    });
}

/// Fills the installed-app catalog in the background.
///
/// The catalog is a process-wide cache that does not live in the broker, so it is built without
/// holding the broker lock: enumerating installed apps spawns Spotlight queries per app and would
/// otherwise block every settings command that needs the broker for seconds.
fn warm_app_search_cache() {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let result = PlatformBroker::warm_app_catalog();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result: Result<(), String> = Ok(());

        if let Err(error) = result {
            eprintln!("failed to warm app search cache: {error}");
        }
    });
}

/// Restarts the overlay loop if it dies or wedges.
///
/// The overlay loop runs on its own thread and restarts itself after a panic
/// (up to a small budget). Once that budget is spent, or when the loop stops
/// heartbeating because it is stuck, [`HighlighterService::is_running`] turns
/// false and this watchdog respawns it.
///
/// The restart is rate-limited: a loop that dies immediately on every start
/// would otherwise become a restart loop.
fn watch_highlighter(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
        const MAX_RESTARTS: u32 = 5;
        const RESTART_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);

        let mut restarts: u32 = 0;
        let mut last_restart = std::time::Instant::now() - RESTART_COOLDOWN;

        loop {
            tokio::time::sleep(WATCH_INTERVAL).await;

            let service = app.state::<HighlighterService>();
            if service.is_running() {
                continue;
            }

            // The user may have turned the service off deliberately.
            let enabled = {
                let config = app.state::<Arc<Mutex<Config>>>();
                let config = config.lock().await;
                config.highlighter_service_enabled
            };
            if !enabled {
                continue;
            }

            let granted = {
                let broker = app.state::<RwLock<PlatformBroker>>();
                broker
                    .read()
                    .map(|broker| {
                        broker.accessibility_permission_status()
                            == AccessibilityPermissionStatus::Granted
                    })
                    .unwrap_or(false)
            };
            if !granted {
                continue;
            }

            if restarts >= MAX_RESTARTS {
                continue;
            }
            if last_restart.elapsed() < RESTART_COOLDOWN {
                continue;
            }

            restarts += 1;
            last_restart = std::time::Instant::now();
            tracing::warn!(restarts, "highlighter stopped; restarting it");
            if let Err(error) = service.start() {
                tracing::warn!(%error, "could not restart the highlighter");
            }
        }
    });
}

pub fn main() {
    let subscriber = FmtSubscriber::builder()
        .map_writer(move |_| stderr)
        .with_ansi(false)
        .with_max_level(if cfg!(debug_assertions) {
            Level::INFO
        } else {
            Level::WARN
        })
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Unable to set up tracing subscriber.");

    // The app takes no subcommands; parsing still rejects stray arguments and serves `--help`.
    let _args = Args::parse();
    run_tauri();
}

pub fn run_tauri() {
    let async_runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build config runtime");

    // Infer
    let is_first_launch = match Config::main_config_exists() {
        Ok(exists) => !exists,
        Err(error) => {
            eprintln!("failed to check config existence: {error}");
            false
        }
    };

    let mut config = if is_first_launch {
        let config = Config::new();

        if let Err(error) = async_runtime.block_on(config.save_to_system()) {
            eprintln!("failed to save initial config: {error}");
        }

        config
    } else {
        match async_runtime.block_on(Config::load_from_system()) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("failed to load config, using defaults: {error}");
                Config::new()
            }
        }
    };

    if config.merge_curated_integrations()
        && let Err(error) = async_runtime.block_on(config.save_to_system())
    {
        eprintln!("failed to save merged integrations: {error}");
    }

    if let Err(error) = personalization_db::sync_and_status(&config) {
        eprintln!("failed to sync personalization sqlite db on startup: {error}");
    }

    let broker = platform_broker(|_| false);

    let accessibility_status = broker.accessibility_permission_status();
    let should_start_highlighter = config.highlighter_service_enabled
        && accessibility_status == AccessibilityPermissionStatus::Granted;
    let onboarding_completed = config.onboarding_completed;
    tracing::info!(
        ?accessibility_status,
        should_start_highlighter,
        "Elocutio desktop starting"
    );
    let config = Arc::new(Mutex::new(config));

    tauri::Builder::default()
        .manage(config.clone())
        .manage(RwLock::new(broker))
        .manage(async_runtime)
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(commands::application_message_handler())
        .setup(move |app| {
            let suggestion_count = std::sync::Arc::new(crate::config::SuggestionCount::default());
            let highlighter_service = HighlighterService::new(suggestion_count.clone());
            elo_overlay::start(app.handle().clone(), config, suggestion_count);
            if should_start_highlighter {
                match highlighter_service.start() {
                    Ok(started) => tracing::info!(started, "elo overlay service started"),
                    Err(err) => error!("Unable to start elo overlay: {err}"),
                }
            }
            app.manage(highlighter_service);

            set_up_tray_menu(app.handle())?;
            register_shortcuts(app.handle());
            #[cfg(target_os = "macos")]
            apple_fm_bridge::start();
            auto_analyze::start(app.handle().clone());
            warm_app_search_cache();
            watch_highlighter(app.handle().clone());

            let _ = is_first_launch;
            let _ = onboarding_completed;
            windows::show_settings_window(app.handle())?;

            #[cfg(target_os = "macos")]
            windows::sync_dock_visibility(app.handle(), None)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &event
            {
                let _ = windows::sync_dock_visibility(_app, Some(label))
                    .inspect_err(|err| error!("Could not update Dock visibility: {err}"));
            }

            // Hand the target app back its accessibility settings before the process goes away.
            if let tauri::RunEvent::Exit = event {
                elo_overlay::shutdown();
            }

            // Keep the tray and service alive after the last window closes, but allow Quit.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
