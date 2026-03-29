#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod config;
mod fetch;
mod insertion;
mod lang;
mod ocr;
mod tray;
mod utils;
mod windows;
mod writing;

use config::get_config;
use get_selected_text::get_selected_text;
use insertion::{
    insert_translation_into_previous_input, remember_active_window, remember_active_window_command,
};
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use serde_json::json;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::{CpuExt, System, SystemExt};
use tauri_plugin_aptabase::EventTracker;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;
use tray::{PinnedFromTrayEvent, PinnedFromWindowEvent};
use windows::{get_translator_window, CheckUpdateEvent, CheckUpdateResultEvent};

use crate::config::{clear_config_cache, get_config_content, ConfigUpdatedEvent};
use crate::fetch::fetch_stream;
use crate::lang::detect_lang;
use crate::ocr::{cut_image, finish_ocr, screenshot, start_ocr};
use crate::windows::{
    get_translator_window_always_on_top, hide_translator_window, show_action_manager_window,
    show_history_window, show_translator_window_command,
    show_translator_window_with_selected_text_command, show_updater_window, TRANSLATOR_WIN_NAME,
};
use crate::writing::{finish_writing, write_to_input, writing_command};

use mouce::{Mouse, MouseActions};
use once_cell::sync::OnceCell;
#[cfg(debug_assertions)]
use specta_typescript::{formatter::prettier, Typescript};
use tauri::{AppHandle, LogicalPosition, LogicalSize};
use tauri::{Manager, PhysicalPosition, PhysicalSize};
use tauri_plugin_notification::NotificationExt;
use tiny_http::{Response as HttpResponse, Server};
use tokio::runtime::{
    Builder as TokioRuntimeBuilder, EnterGuard as TokioEnterGuard, Runtime as TokioRuntime,
};

pub static APP_HANDLE: OnceCell<AppHandle> = OnceCell::new();
pub static ALWAYS_ON_TOP: AtomicBool = AtomicBool::new(false);
pub static CPU_VENDOR: Mutex<String> = Mutex::new(String::new());
pub static SELECTED_TEXT: Mutex<String> = Mutex::new(String::new());
pub static PREVIOUS_PRESS_TIME: Mutex<u128> = Mutex::new(0);
pub static PREVIOUS_RELEASE_TIME: Mutex<u128> = Mutex::new(0);
pub static PREVIOUS_RELEASE_POSITION: Mutex<(i32, i32)> = Mutex::new((0, 0));
pub static RELEASE_THREAD_ID: Mutex<u32> = Mutex::new(0);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    version: String,
    current_version: String,
    body: Option<String>,
}

pub static UPDATE_RESULT: Mutex<Option<Option<UpdateResult>>> = Mutex::new(None);

/// Set up a global panic hook that writes crash details to a file before the
/// process terminates. This captures panics from ALL threads, not just main.
fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let timestamp = chrono_local_timestamp();

        let crash_report = format!(
            "=== CRASH REPORT ===\n\
             Timestamp: {}\n\
             Thread: {}\n\
             Location: {}\n\
             Message: {}\n\
             \n\
             Backtrace:\n\
             {}\n\
             ===================\n\n",
            timestamp, thread_name, location, message, backtrace
        );

        // Write to crash.log synchronously — must complete before process exits
        if let Some(crash_path) = get_crash_log_path() {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&crash_path)
            {
                use std::io::Write;
                let _ = file.write_all(crash_report.as_bytes());
                let _ = file.flush();
            }
        }

        // Also log via the log crate (if the logger is initialized)
        error!(
            "PANIC in thread '{}' at {}: {}",
            thread_name, location, message
        );

        // Forward to Aptabase if available
        if let Some(handle) = APP_HANDLE.get() {
            let _ = handle.track_event(
                "panic",
                Some(json!({
                    "info": format!("{} ({})", message, location),
                    "thread": thread_name,
                })),
            );
            handle.flush_events_blocking();
        }

        // Call the default hook for stderr output
        default_hook(info);
    }));
}

/// Get a local timestamp string without pulling in the `chrono` crate.
fn chrono_local_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            let millis = d.subsec_millis();
            format!("{}s+{}ms (unix epoch)", secs, millis)
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Resolve the crash.log path under the app config directory.
fn get_crash_log_path() -> Option<std::path::PathBuf> {
    if let Some(handle) = APP_HANDLE.get() {
        if let Ok(dir) = handle
            .path()
            .resolve("", tauri::path::BaseDirectory::AppLog)
        {
            let _ = std::fs::create_dir_all(&dir);
            return Some(dir.join("crash.log"));
        }
    }
    // Fallback: try standard AppData path on Windows
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let dir =
                std::path::PathBuf::from(appdata).join("xyz.yetone.apps.openai-translator");
            let _ = std::fs::create_dir_all(&dir);
            return Some(dir.join("crash.log"));
        }
    }
    None
}

fn init_tokio_runtime() -> &'static TokioRuntime {
    use std::sync::OnceLock;

    static RUNTIME: OnceLock<&'static TokioRuntime> = OnceLock::new();
    static ENTER_GUARD: OnceLock<&'static TokioEnterGuard<'static>> = OnceLock::new();
    static SET_HANDLE: OnceLock<()> = OnceLock::new();

    let runtime = RUNTIME.get_or_init(|| {
        Box::leak(Box::new(
            TokioRuntimeBuilder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to initialize Tokio runtime"),
        ))
    });

    ENTER_GUARD.get_or_init(|| Box::leak(Box::new(runtime.enter())));

    if SET_HANDLE.set(()).is_ok() {
        tauri::async_runtime::set(runtime.handle().clone());
    }

    runtime
}

#[tauri::command]
#[specta::specta]
fn get_update_result() -> (bool, Option<UpdateResult>) {
    if UPDATE_RESULT.lock().is_none() {
        return (false, None);
    }
    return (true, UPDATE_RESULT.lock().clone().unwrap());
}
#[cfg(target_os = "macos")]
fn query_accessibility_permissions() -> bool {
    let trusted = macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
    if trusted {
        info!("Application is totally trusted!");
    } else {
        warn!("Application isn't trusted for accessibility");
    }
    trusted
}

#[cfg(not(target_os = "macos"))]
fn query_accessibility_permissions() -> bool {
    return true;
}

#[inline]
fn launch_ipc_server(server: &Server) {
    for mut req in server.incoming_requests() {
        let mut selected_text = String::new();
        if let Err(e) = req.as_reader().read_to_string(&mut selected_text) {
            error!("IPC server: failed to read request body: {:?}", e);
            continue;
        }
        utils::send_text(selected_text);
        remember_active_window();
        let window = windows::show_translator_window(false, true, false);
        if let Err(e) = window.set_focus() {
            warn!("IPC server: failed to set window focus: {:?}", e);
        }
        utils::show();
        let response = HttpResponse::from_string("ok");
        if let Err(e) = req.respond(response) {
            warn!("IPC server: failed to send response: {:?}", e);
        }
    }
}

fn bind_mouse_hook() {
    if !query_accessibility_permissions() {
        return;
    }

    // Mouse event hook requires `sudo` permission on linux.
    // Let's just skip it.
    if cfg!(target_os = "linux") {
        info!("Mouse event hook skipped on Linux");
        return;
    }

    let mut mouse_manager = Mouse::new();

    let hook_result = mouse_manager.hook(Box::new(|event| {
        match event {
            mouce::common::MouseEvent::Press(mouce::common::MouseButton::Left) => {
                let config = match config::get_config() {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Mouse press: failed to get config: {:?}", e);
                        return;
                    }
                };
                let always_show_icons = config.always_show_icons.unwrap_or(true);
                if !always_show_icons {
                    return;
                }
                let current_press_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                *PREVIOUS_PRESS_TIME.lock() = current_press_time;
            }
            mouce::common::MouseEvent::Release(mouce::common::MouseButton::Left) => {
                let config = match config::get_config() {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Mouse release: failed to get config: {:?}", e);
                        return;
                    }
                };
                let always_show_icons = config.always_show_icons.unwrap_or(true);
                if !always_show_icons {
                    windows::delete_thumb();
                    return;
                }
                let current_release_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                let mut is_text_selected_event = false;
                let (x, y): (i32, i32) = match windows::get_mouse_location() {
                    Ok(pos) => pos,
                    Err(e) => {
                        warn!("Mouse release: failed to get mouse location: {:?}", e);
                        return;
                    }
                };
                let (prev_release_x, prev_release_y) = { *PREVIOUS_RELEASE_POSITION.lock() };
                {
                    *PREVIOUS_RELEASE_POSITION.lock() = (x, y);
                }
                let mouse_distance =
                    (((x - prev_release_x).pow(2) + (y - prev_release_y).pow(2)) as f64).sqrt();
                let previous_press_time: u128;
                let previous_release_time: u128;
                {
                    let previous_press_time_lock = PREVIOUS_PRESS_TIME.lock();
                    let mut previous_release_time_lock = PREVIOUS_RELEASE_TIME.lock();
                    previous_release_time = *previous_release_time_lock;
                    *previous_release_time_lock = current_release_time;
                    previous_press_time = *previous_press_time_lock;
                }
                let is_pressed = previous_release_time < previous_press_time;
                let pressed_time = current_release_time - previous_press_time;
                let is_double_click =
                    current_release_time - previous_release_time < 700 && mouse_distance < 10.0;
                if is_pressed && pressed_time > 300 && mouse_distance > 20.0 {
                    is_text_selected_event = true;
                }
                if previous_release_time != 0 && is_double_click {
                    is_text_selected_event = true;
                }
                let is_click_on_thumb = match APP_HANDLE.get() {
                    Some(handle) => match handle.get_webview_window(windows::THUMB_WIN_NAME) {
                        Some(window) => match window.outer_position() {
                            Ok(position) => {
                                let scale_factor = window.scale_factor().unwrap_or(1.0);
                                if let Ok(size) = window.outer_size() {
                                    if cfg!(target_os = "macos") {
                                        let LogicalPosition { x: x1, y: y1 } =
                                            position.to_logical::<i32>(scale_factor);
                                        let LogicalSize {
                                            width: mut w,
                                            height: mut h,
                                        } = size.to_logical::<i32>(scale_factor);
                                        if cfg!(target_os = "windows") {
                                            w = (20.0 as f64 * scale_factor) as i32;
                                            h = (20.0 as f64 * scale_factor) as i32;
                                        }
                                        let (x2, y2) = (x1 + w, y1 + h);
                                        let res = x >= x1 && x <= x2 && y >= y1 && y <= y2;
                                        res
                                    } else {
                                        let PhysicalPosition { x: x1, y: y1 } = position;
                                        let PhysicalSize {
                                            width: mut w,
                                            height: mut h,
                                        } = size;
                                        if cfg!(target_os = "windows") {
                                            w = (20.0 as f64 * scale_factor) as u32;
                                            h = (20.0 as f64 * scale_factor) as u32;
                                        }
                                        let (x2, y2) = (x1 + w as i32, y1 + h as i32);
                                        let res = x >= x1 && x <= x2 && y >= y1 && y <= y2;
                                        res
                                    }
                                } else {
                                    false
                                }
                            }
                            Err(err) => {
                                debug!("Thumb window position error: {:?}", err);
                                false
                            }
                        },
                        None => false,
                    },
                    None => false,
                };
                if !is_text_selected_event && !is_click_on_thumb {
                    windows::close_thumb();
                    return;
                }

                if !is_click_on_thumb {
                    if RELEASE_THREAD_ID.is_locked() {
                        return;
                    }
                    std::thread::spawn(move || {
                        #[cfg(target_os = "macos")]
                        {
                            if !utils::is_valid_selected_frame().unwrap_or(false) {
                                debug!("No valid selected frame");
                                windows::close_thumb();
                                return;
                            }
                        }

                        let _lock = RELEASE_THREAD_ID.lock();
                        let selected_text = get_selected_text().unwrap_or_default();
                        if !selected_text.is_empty() {
                            {
                                *SELECTED_TEXT.lock() = selected_text;
                            }
                            windows::show_thumb(x, y);
                        } else {
                            windows::close_thumb();
                        }
                    });
                } else {
                    windows::close_thumb();
                    let selected_text = (*SELECTED_TEXT.lock()).to_string();
                    if !selected_text.is_empty() {
                        remember_active_window();
                        let window = windows::show_translator_window(false, true, false);
                        utils::send_text(selected_text);
                        if let Err(e) = window.set_focus() {
                            warn!("Failed to set translator window focus: {:?}", e);
                        }
                    }
                }
            }
            _ => {}
        }
    }));

    match hook_result {
        Ok(_) => {
            info!("Mouse event hook installed successfully");
        }
        Err(e) => {
            error!("Failed to install mouse event hook: {}", e);
        }
    }
}

fn main() {
    setup_panic_hook();
    let _ = init_tokio_runtime();
    let silently = env::args().any(|arg| arg == "--silently");

    let mut sys = System::new();
    sys.refresh_cpu(); // Refreshing CPU information.
    if let Some(cpu) = sys.cpus().first() {
        let vendor_id = cpu.vendor_id().to_string();
        *CPU_VENDOR.lock() = vendor_id;
    }

    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            get_config_content,
            get_update_result,
            clear_config_cache,
            show_translator_window_command,
            show_translator_window_with_selected_text_command,
            show_action_manager_window,
            show_history_window,
            get_translator_window_always_on_top,
            fetch_stream,
            writing_command,
            write_to_input,
            finish_writing,
            insert_translation_into_previous_input,
            remember_active_window_command,
            detect_lang,
            screenshot,
            hide_translator_window,
            start_ocr,
            finish_ocr,
            cut_image,
        ])
        .events(tauri_specta::collect_events![
            CheckUpdateEvent,
            CheckUpdateResultEvent,
            PinnedFromWindowEvent,
            PinnedFromTrayEvent,
            ConfigUpdatedEvent
        ]);

    #[cfg(debug_assertions)]
    specta_builder
        .export(
            Typescript::default().formatter(prettier),
            "../src/tauri/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    let invoke_handler = specta_builder.invoke_handler();
    let specta_builder = Arc::new(specta_builder);

    let specta_builder_setup = specta_builder.clone();

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut app = tauri::Builder::default()
        .plugin(
            tauri_plugin_aptabase::Builder::new("A-US-9856842764").build(),
        )
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("app".into()),
                    }),
                ])
                .max_file_size(5_000_000) // 5MB rotation
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            info!(
                "Single instance detected: {}, argv={:?}, cwd={}",
                app.package_info().name,
                argv,
                cwd
            );
            app.notification()
                .builder()
                .title("This app is already running!")
                .body("You can find it in the tray menu.")
                .show()
                .unwrap();
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--silently"]),
        ))
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            info!("App setup started");
            specta_builder_setup.mount_events(app);
            let app_handle = app.handle();
            APP_HANDLE.get_or_init(|| app.handle().clone());
            tray::create_tray(&app_handle)?;
            app_handle.plugin(tauri_plugin_global_shortcut::Builder::new().build())?;
            app_handle.plugin(tauri_plugin_updater::Builder::new().build())?;
            // create thumb window
            let _ = windows::get_thumb_window(0, 0);
            if silently {
                // create translator window
                let _ = get_translator_window(false, false, false);
                windows::do_hide_translator_window();
                debug!("Translator window created (hidden, silently mode)");
            } else {
                let window = get_translator_window(false, false, false);
                window.set_focus().unwrap();
                window.show().unwrap();
            }
            if !query_accessibility_permissions() {
                if let Some(window) = app.get_webview_window(TRANSLATOR_WIN_NAME) {
                    window.minimize().unwrap();
                }
                app.notification()
                    .builder()
                    .title("Accessibility permissions")
                    .body("Please grant accessibility permissions to the app")
                    .icon("icon.png")
                    .show()
                    .unwrap();
            }
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    #[cfg(target_os = "windows")]
                    {
                        match Server::http("127.0.0.1:62007") {
                            Ok(server) => {
                                info!("IPC server listening on 127.0.0.1:62007");
                                launch_ipc_server(&server);
                            }
                            Err(e) => {
                                error!("Failed to bind IPC server on port 62007: {:?}", e);
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        use std::path::Path;
                        let path = Path::new("/tmp/openai-translator.sock");
                        std::fs::remove_file(path).unwrap_or_default();
                        match Server::http_unix(path) {
                            Ok(server) => {
                                info!("IPC server listening on /tmp/openai-translator.sock");
                                launch_ipc_server(&server);
                            }
                            Err(e) => {
                                error!("Failed to bind IPC unix socket: {:?}", e);
                            }
                        }
                    }
                });
                if let Err(e) = result {
                    error!("IPC server thread panicked: {:?}", e);
                }
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60 * 10));
                    info!("Running periodic update check");
                    let builder = handle.updater_builder();
                    let updater = match builder.build() {
                        Ok(u) => u,
                        Err(e) => {
                            error!("Failed to build updater: {:?}", e);
                            continue;
                        }
                    };

                    match updater.check().await {
                        Ok(Some(update)) => {
                            info!("Update available: v{}", update.version);
                            *UPDATE_RESULT.lock() = Some(Some(UpdateResult {
                                version: update.version,
                                current_version: update.current_version,
                                body: update.body,
                            }));
                            if let Err(e) = tray::create_tray(&handle) {
                                error!("Failed to update tray after update check: {:?}", e);
                            }
                        }
                        Ok(None) => {
                            if UPDATE_RESULT.lock().is_some() {
                                if let Some(Some(_)) = *UPDATE_RESULT.lock() {
                                    *UPDATE_RESULT.lock() = Some(None);
                                    if let Err(e) = tray::create_tray(&handle) {
                                        error!("Failed to update tray: {:?}", e);
                                    }
                                }
                            } else {
                                *UPDATE_RESULT.lock() = Some(None);
                            }
                        }
                        Err(e) => {
                            warn!("Periodic update check failed: {:?}", e);
                        }
                    }
                }
            });
            let handle = app_handle.clone();
            PinnedFromWindowEvent::listen_any(app_handle, move |event| {
                let pinned = event.payload.pinned();
                ALWAYS_ON_TOP.store(*pinned, Ordering::Release);
                if let Err(e) = tray::create_tray(&handle) {
                    error!("Failed to update tray on pin event: {:?}", e);
                }
            });

            let handle = app_handle.clone();
            ConfigUpdatedEvent::listen_any(app_handle, move |_event| {
                clear_config_cache();
                if let Err(e) = tray::create_tray(&handle) {
                    error!("Failed to update tray on config change: {:?}", e);
                }
            });
            info!("App setup completed");
            Ok(())
        })
        .invoke_handler(invoke_handler)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    #[cfg(target_os = "macos")]
    {
        let config = config::get_config_by_app(app.handle()).unwrap();
        if config.hide_the_icon_in_the_dock.unwrap_or(true) {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        } else {
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
        }
    }

    app.run(|app, event| match event {
        tauri::RunEvent::Exit { .. } => {
            info!("App exiting");
            let _ = app.track_event("app_exited", None);
            app.flush_events_blocking();
        }
        tauri::RunEvent::Ready => {
            info!("App ready");
            let _ = app.track_event("app_started", None);
            bind_mouse_hook();
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                info!("Running initial update check");
                let builder = handle.updater_builder();
                let updater = match builder.build() {
                    Ok(u) => u,
                    Err(e) => {
                        error!("Failed to build updater on ready: {:?}", e);
                        return;
                    }
                };

                match updater.check().await {
                    Ok(Some(update)) => {
                        info!("Update available on startup: v{}", update.version);
                        *UPDATE_RESULT.lock() = Some(Some(UpdateResult {
                            version: update.version,
                            current_version: update.current_version,
                            body: update.body,
                        }));
                        if let Err(e) = tray::create_tray(&handle) {
                            error!("Failed to update tray on startup: {:?}", e);
                        }
                        let config = match get_config() {
                            Ok(c) => c,
                            Err(e) => {
                                warn!("Failed to get config for update check: {:?}", e);
                                return;
                            }
                        };
                        if config.automatic_check_for_updates.is_none()
                            || config
                                .automatic_check_for_updates
                                .is_some_and(|x| x == true)
                        {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            show_updater_window();
                        }
                    }
                    Ok(None) => {
                        if UPDATE_RESULT.lock().is_some() {
                            if let Some(Some(_)) = *UPDATE_RESULT.lock() {
                                *UPDATE_RESULT.lock() = Some(None);
                                if let Err(e) = tray::create_tray(&handle) {
                                    error!("Failed to update tray on startup: {:?}", e);
                                }
                            }
                        } else {
                            *UPDATE_RESULT.lock() = Some(None);
                        }
                    }
                    Err(e) => {
                        warn!("Initial update check failed: {:?}", e);
                    }
                }
            });
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } => {
            if label != TRANSLATOR_WIN_NAME {
                return;
            }

            windows::do_hide_translator_window();

            api.prevent_close();
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                remember_active_window();
                windows::show_translator_window(false, false, false);
            }
        }
        _ => {}
    });
}
