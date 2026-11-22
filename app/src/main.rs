// The window around ram-core: commands in, events out, PDF pages over the ram:// protocol.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ram_core::engine::{DeviceList, Engine, LessonInfo, Out, Paths};
use ram_core::settings::Settings;
use ram_core::session::Cmd;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::http::{Response, StatusCode};
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, State};

struct App(Arc<Engine>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Added {
    added: usize,
    unpaired_audio: Vec<String>,
    unpaired_pdf: Vec<String>,
}

#[tauri::command]
fn lessons(app: State<App>) -> Vec<LessonInfo> {
    app.0.lessons()
}

/// Off the main thread: a big folder or zip takes a moment, and the window keeps drawing meanwhile.
#[tauri::command]
async fn add_paths(app: State<'_, App>, paths: Vec<String>) -> Result<Added, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let e = app.0.clone();
    let (added, rest) = tauri::async_runtime::spawn_blocking(move || e.add(&paths)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let s = |v: Vec<PathBuf>| v.into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
    Ok(Added { added, unpaired_audio: s(rest.audio), unpaired_pdf: s(rest.pdf) })
}

#[tauri::command]
fn set_pdf(app: State<App>, audio: String, pdf: String) -> Result<(), String> {
    app.0.library.lock().set_pdf(audio.as_ref(), pdf.into()).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_lesson(app: State<App>, audio: String) -> Result<(), String> {
    app.0.library.lock().remove(audio.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_lesson(app: State<App>, id: String) {
    app.0.open(&id);
}

#[tauri::command]
fn command(app: State<App>, cmd: Cmd) {
    app.0.command(cmd);
}

#[tauri::command]
fn reset_lesson(app: State<App>, id: String) {
    app.0.reset_lesson(&id);
}

#[tauri::command]
async fn reset_app(app: State<'_, App>) -> Result<(), String> {
    let e = app.0.clone();
    tauri::async_runtime::spawn_blocking(move || e.reset_app()).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_all(app: State<App>) {
    app.0.reset_all();
}

#[tauri::command]
fn clear_marks(app: State<App>, id: String) {
    app.0.clear_marks(&id);
}

#[tauri::command]
async fn rescan(app: State<'_, App>) -> Result<usize, String> {
    let e = app.0.clone();
    tauri::async_runtime::spawn_blocking(move || e.rescan()).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_group(app: State<App>, group: String) -> Result<(), String> {
    app.0.remove_group(&group).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_group(app: State<App>, from: String, to: String) -> Result<(), String> {
    app.0.rename_group(&from, &to).map_err(|e| e.to_string())
}

#[tauri::command]
fn backup_progress(app: State<App>, path: String) -> Result<usize, String> {
    app.0.backup_progress(path.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_progress(app: State<'_, App>, path: String) -> Result<usize, String> {
    let e = app.0.clone();
    tauri::async_runtime::spawn_blocking(move || e.restore_progress(path.as_ref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(app: State<App>) -> Settings {
    app.0.settings()
}

#[tauri::command]
fn set_settings(app: State<App>, settings: Settings) -> Result<(), String> {
    app.0.set_settings(settings).map_err(|e| e.to_string())
}

#[tauri::command]
async fn devices(app: State<'_, App>) -> Result<DeviceList, String> {
    Ok(app.0.devices())
}

#[tauri::command]
async fn mic_check(app: State<'_, App>) -> Result<ram_core::device::MicState, String> {
    let e = app.0.clone();
    tauri::async_runtime::spawn_blocking(move || e.mic_check()).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_logs(app: State<'_, App>, path: String) -> Result<usize, String> {
    app.0.export_logs(path.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn logs_dir(app: State<App>) -> String {
    let d = app.0.logs_dir();
    let _ = std::fs::create_dir_all(&d);
    d.to_string_lossy().into_owned()
}

#[tauri::command]
fn mark(app: State<App>, words: Vec<usize>) {
    app.0.mark(words);
}

#[tauri::command]
fn mark_phrase(app: State<App>) {
    app.0.mark_phrase();
}

#[tauri::command]
async fn export_answers(app: State<'_, App>, path: String) -> Result<usize, String> {
    let e = app.0.clone();
    tauri::async_runtime::spawn_blocking(move || e.export_answers(path.as_ref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Errors in the window's code end up in logs/app.log too.
#[tauri::command]
fn log_ui(message: String) {
    log::error!("ui: {message}");
}

#[tauri::command]
fn test_sound(app: State<App>) {
    app.0.test_sound();
}

#[tauri::command]
fn mic_test(app: State<App>, on: bool) {
    app.0.mic_test(on);
}

#[tauri::command]
fn data_dir(app: State<App>) -> String {
    app.0.data_dir().to_string_lossy().into_owned()
}

fn data_dir_for(handle: &AppHandle) -> PathBuf {
    if let Some(d) = std::env::var_os("RAM_DATA_DIR") {
        return d.into(); // a separate place for testing, never your real progress
    }
    handle.path().app_data_dir().expect("no data folder")
}

/// Where the bundled files are: next to the app when installed, in vendor/ when developing.
fn resource(handle: &AppHandle, rel: &str, dev: &str) -> PathBuf {
    let installed = handle.path().resource_dir().map(|d| d.join(rel)).ok();
    match installed {
        Some(p) if p.exists() => p,
        // the source folder's path is only written into development builds, never into a release
        #[cfg(debug_assertions)]
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(dev),
        #[cfg(not(debug_assertions))]
        _ => PathBuf::from(dev),
    }
}

/// macOS keeps the library in Contents/Frameworks (signed with the app); elsewhere it's a resource.
fn pdfium_dir(handle: &AppHandle) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe() {
        let fw = exe.parent().map(|d| d.join("../Frameworks")).unwrap_or_default();
        if fw.join("libpdfium.dylib").exists() {
            return fw;
        }
    }
    resource(handle, "pdfium", pdfium_dev_dir())
}

fn pdfium_dev_dir() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "vendor/pdfium/mac-arm64/lib"
    } else if cfg!(target_os = "macos") {
        "vendor/pdfium/mac-x64/lib"
    } else if cfg!(target_os = "windows") {
        "vendor/pdfium/win-x64/bin"
    } else {
        "vendor/pdfium/linux-x64/lib"
    }
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let add_files = MenuItem::with_id(app, "add-files", "Add Lesson Files…", true, Some("CmdOrCtrl+O"))?;
    let add_folder = MenuItem::with_id(app, "add-folder", "Add a Folder of Lessons…", true, Some("CmdOrCtrl+Shift+O"))?;
    let reveal = MenuItem::with_id(app, "data", "Show Saved Progress", true, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", "Start This Lesson Over…", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Look for New Lessons", true, None::<&str>)?;
    let backup = MenuItem::with_id(app, "backup", "Back Up Progress…", true, None::<&str>)?;
    let restore = MenuItem::with_id(app, "restore", "Restore Progress…", true, None::<&str>)?;
    let keys = MenuItem::with_id(app, "keys", "Keyboard Shortcuts", true, Some("CmdOrCtrl+/"))?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let zoom_in = MenuItem::with_id(app, "zoom-in", "Zoom In", true, Some("CmdOrCtrl+="))?;
    let zoom_out = MenuItem::with_id(app, "zoom-out", "Zoom Out", true, Some("CmdOrCtrl+-"))?;
    let zoom_fit = MenuItem::with_id(app, "zoom-fit", "Fit Width", true, Some("CmdOrCtrl+0"))?;
    let lessons = MenuItem::with_id(app, "lessons", "Lessons", true, Some("CmdOrCtrl+L"))?;
    #[cfg(target_os = "macos")]
    let file = Submenu::with_items(app, "File", true, &[&add_files, &add_folder, &rescan, &PredefinedMenuItem::separator(app)?, &reset, &PredefinedMenuItem::separator(app)?, &backup, &restore, &reveal, &PredefinedMenuItem::separator(app)?, &PredefinedMenuItem::close_window(app, None)?])?;
    // (no app menu on Windows/Linux: Settings goes in File)
    #[cfg(not(target_os = "macos"))]
    let file = Submenu::with_items(app, "File", true, &[&add_files, &add_folder, &rescan, &PredefinedMenuItem::separator(app)?, &reset, &PredefinedMenuItem::separator(app)?, &backup, &restore, &reveal, &PredefinedMenuItem::separator(app)?, &settings, &PredefinedMenuItem::separator(app)?, &PredefinedMenuItem::quit(app, None)?])?;
    let edit = Submenu::with_items(app, "Edit", true, &[&PredefinedMenuItem::copy(app, None)?, &PredefinedMenuItem::select_all(app, None)?])?;
    let view = Submenu::with_items(app, "View", true, &[&lessons, &PredefinedMenuItem::separator(app)?, &zoom_in, &zoom_out, &zoom_fit, &PredefinedMenuItem::separator(app)?, &PredefinedMenuItem::fullscreen(app, None)?])?;
    let help = Submenu::with_items(app, "Help", true, &[&keys])?;
    #[cfg(target_os = "macos")]
    {
        let about = AboutMetadata { name: Some("Repeat After Me".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
        let app_menu = Submenu::with_items(app, "Repeat After Me", true, &[
            &PredefinedMenuItem::about(app, None, Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ])?;
        return Menu::with_items(app, &[&app_menu, &file, &edit, &view, &help]);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = AboutMetadata::default();
        Menu::with_items(app, &[&file, &edit, &view, &help])
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let data = data_dir_for(&handle);
            // errors and warnings go to logs/app.log (Settings → Export logs)
            let logs = data.join("logs");
            let _ = std::fs::create_dir_all(&logs);
            let mut lb = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"));
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(logs.join("app.log")) {
                lb.target(env_logger::Target::Pipe(Box::new(f)));
            }
            let _ = lb.try_init();
            log::warn!("started {}", env!("CARGO_PKG_VERSION"));
            let paths = Paths {
                data,
                model: resource(&handle, "models/ggml-small.en-q5_1.bin", "vendor/models/ggml-small.en-q5_1.bin"),
                pdfium_dir: Some(pdfium_dir(&handle)),
            };
            let h = handle.clone();
            let sink = Arc::new(move |o: Out| {
                let _ = h.emit("ram", o);
            });
            let engine = Engine::start(paths, sink).map_err(|e| e.to_string())?;
            app.manage(App(Arc::new(engine)));
            app.set_menu(menu(&handle)?)?;
            app.on_menu_event(|app, ev| {
                let _ = app.emit("menu", ev.id().0.as_str());
            });
            Ok(())
        })
        // ram://localhost/page/<lesson id>/<page>?w=<pixels>  →  PNG
        .register_asynchronous_uri_scheme_protocol("ram", |ctx, req, responder| {
            let engine = ctx.app_handle().state::<App>().0.clone();
            let uri = req.uri().clone();
            std::thread::spawn(move || {
                let path = percent_decode(uri.path());
                let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
                let width = uri
                    .query()
                    .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("w=")))
                    .and_then(|w| w.parse().ok())
                    .unwrap_or(1200);
                let res = match parts.as_slice() {
                    ["page", id, page] => page.parse().map_err(|_| "bad page".to_string()).and_then(|p| engine.page_png(id, p, width).map_err(|e| e.to_string())),
                    _ => Err("not found".into()),
                };
                let resp = match res {
                    Ok(png) => Response::builder()
                        .header("Content-Type", "image/png")
                        .header("Cache-Control", "max-age=3600")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(png),
                    Err(e) => Response::builder().status(StatusCode::NOT_FOUND).body(e.into_bytes()),
                };
                responder.respond(resp.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![lessons, add_paths, set_pdf, remove_lesson, open_lesson, command, reset_lesson, reset_all, reset_app, clear_marks, rescan, remove_group, rename_group, backup_progress, restore_progress, data_dir, get_settings, set_settings, devices, mic_test, mic_check, test_sound, export_logs, logs_dir, log_ui, mark, mark_phrase, export_answers])
        .run(tauri::generate_context!())
        .expect("error while running Repeat After Me");
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
