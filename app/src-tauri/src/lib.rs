//! Tauri command layer over `godsend-core`.
//!
//! The frontend calls these with `invoke(...)`. Long work runs in the job
//! queue and reports through `job://progress` and `job://job`.

use godsend_core::config::{Config, Host};
use godsend_core::convert;
use godsend_core::device;
use godsend_core::doctor;
use godsend_core::fm;
use godsend_core::ghostpad::Ghostpad;
use godsend_core::homebrew::{self, CatalogEntry};
use godsend_core::install;
use godsend_core::library::{self, GameEntry, LibraryView};
use godsend_core::pad;
use godsend_core::sources::{self, GameHit, IaFile, IaItem};
use godsend_core::store;
use godsend_core::transfer::{set_progress_hook, Job, JobContext, JobManager};
use godsend_core::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    jobs: JobManager,
    config: Mutex<Config>,
    pad: Mutex<Option<Ghostpad>>,
}

impl AppState {
    fn new() -> Self {
        let jobs = JobManager::new();
        jobs.load_journal();
        Self {
            jobs,
            config: Mutex::new(Config::load()),
            pad: Mutex::new(None),
        }
    }

    fn config(&self) -> Config {
        self.config.lock().expect("config lock").clone()
    }

    fn host(&self) -> Result<Host> {
        self.config().require_active()
    }

    fn replace_config(&self, next: Config) -> Result<Config> {
        next.save()?;
        *self.config.lock().expect("config lock") = next.clone();
        Ok(next)
    }
}

#[derive(Serialize)]
struct Snapshot {
    hosts: Vec<Host>,
    active: Option<String>,
    config: Config,
    doctor: Vec<doctor::Check>,
    statuses: Vec<device::DeviceStatus>,
    jobs: Vec<Job>,
}

#[tauri::command]
async fn snapshot(state: State<'_, AppState>) -> Result<Snapshot> {
    let config = state.config();
    let statuses = device::probe_all(&config.hosts);
    Ok(Snapshot {
        hosts: config.hosts.clone(),
        active: config.active_host.clone(),
        doctor: doctor::run_with_statuses(&config, &statuses),
        statuses,
        jobs: state.jobs.list(),
        config,
    })
}

#[tauri::command]
async fn probe_host(state: State<'_, AppState>, id: Option<String>) -> Result<device::DeviceStatus> {
    let config = state.config();
    let host = match id {
        Some(id) => config
            .hosts
            .into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| Error::NotFound(format!("no host {id}")))?,
        None => config.require_active()?,
    };
    Ok(device::probe(&host))
}

#[tauri::command]
async fn probe_all(state: State<'_, AppState>) -> Result<Vec<device::DeviceStatus>> {
    Ok(device::probe_all(&state.config().hosts))
}

#[tauri::command]
async fn discover(subnet: String) -> Result<Vec<Host>> {
    device::discover(if subnet.trim().is_empty() {
        "192.168.1"
    } else {
        subnet.trim()
    })
}

#[tauri::command]
async fn host_add(
    state: State<'_, AppState>,
    host: Host,
    replace_id: Option<String>,
) -> Result<Config> {
    let mut config = state.config();
    config.upsert_host_replacing(host, replace_id.as_deref());
    state.replace_config(config)
}

#[tauri::command]
async fn host_remove(state: State<'_, AppState>, id: String) -> Result<Config> {
    let mut config = state.config();
    config.remove_host(&id);
    state.replace_config(config)
}

#[tauri::command]
async fn host_set_active(state: State<'_, AppState>, id: String) -> Result<Config> {
    let mut config = state.config();
    if !config.hosts.iter().any(|h| h.id == id) {
        return Err(Error::NotFound(format!("no host {id}")));
    }
    config.active_host = Some(id);
    state.replace_config(config)
}

#[tauri::command]
async fn library_scan(state: State<'_, AppState>) -> Result<LibraryView> {
    library::scan(&state.host()?)
}

#[tauri::command]
async fn icon(state: State<'_, AppState>, path: String) -> Result<Option<String>> {
    let host = state.host()?;
    Ok(library::icon_bytes(&host, &path)
        .map(|bytes| format!("data:image/png;base64,{}", base64_encode(&bytes))))
}

#[tauri::command]
async fn library_delete(state: State<'_, AppState>, path: String, is_dir: bool) -> Result<()> {
    library::delete_entry(&state.host()?, &path, is_dir)
}

// ---- virtual controller ----

#[derive(Serialize)]
struct PadStatus {
    connected: bool,
    addr: String,
    buttons: u32,
    lx: u8,
    ly: u8,
    rx: u8,
    ry: u8,
    l2: u8,
    r2: u8,
}

impl PadStatus {
    fn off() -> Self {
        Self {
            connected: false,
            addr: String::new(),
            buttons: 0,
            lx: 128,
            ly: 128,
            rx: 128,
            ry: 128,
            l2: 0,
            r2: 0,
        }
    }
}

/// Run one pad operation, connecting on first use and dropping the session
/// when the payload closes it.
fn pad_op<T>(
    state: &AppState,
    op: impl FnOnce(&Ghostpad) -> Result<T>,
) -> Result<T> {
    let host = state.host()?;
    let mut guard = state.pad.lock().expect("pad lock");
    if guard.as_ref().map(|pad| !pad.alive()).unwrap_or(false) {
        *guard = None;
    }
    if guard.is_none() {
        *guard = Some(Ghostpad::connect(&host)?);
    }
    let pad_ref = guard.as_ref().expect("pad session");
    match op(pad_ref) {
        Ok(value) => Ok(value),
        Err(error) => {
            // A bad button name is not a dead session; only drop when the
            // payload actually closed.
            if !pad_ref.alive() {
                *guard = None;
            }
            Err(error)
        }
    }
}

#[tauri::command]
async fn pad_connect(state: State<'_, AppState>) -> Result<PadStatus> {
    let host = state.host()?;
    let mut guard = state.pad.lock().expect("pad lock");
    *guard = Some(Ghostpad::connect(&host)?);
    let state_now = guard.as_ref().expect("pad session").state();
    Ok(PadStatus {
        connected: true,
        addr: guard.as_ref().expect("pad session").addr().to_string(),
        buttons: state_now.buttons,
        lx: state_now.lx,
        ly: state_now.ly,
        rx: state_now.rx,
        ry: state_now.ry,
        l2: state_now.l2,
        r2: state_now.r2,
    })
}

#[tauri::command]
async fn pad_status(state: State<'_, AppState>) -> Result<PadStatus> {
    let Ok(mut guard) = state.pad.lock() else {
        return Ok(PadStatus::off());
    };
    if guard.as_ref().map(|pad| !pad.alive()).unwrap_or(false) {
        *guard = None;
    }
    let Some(session) = guard.as_ref() else {
        return Ok(PadStatus::off());
    };
    let now = session.state();
    Ok(PadStatus {
        connected: true,
        addr: session.addr().to_string(),
        buttons: now.buttons,
        lx: now.lx,
        ly: now.ly,
        rx: now.rx,
        ry: now.ry,
        l2: now.l2,
        r2: now.r2,
    })
}

#[tauri::command]
fn pad_disconnect(state: State<'_, AppState>) {
    if let Ok(mut guard) = state.pad.lock() {
        *guard = None;
    }
}

#[tauri::command]
async fn pad_press(state: State<'_, AppState>, button: String) -> Result<()> {
    pad_op(&state, |pad| pad::press(pad, &button))
}

#[tauri::command]
async fn pad_hold(state: State<'_, AppState>, button: String) -> Result<()> {
    pad_op(&state, |pad| pad::hold(pad, &button))
}

#[tauri::command]
async fn pad_release(state: State<'_, AppState>, button: Option<String>) -> Result<()> {
    match button {
        Some(name) => pad_op(&state, |pad| pad::release_button(pad, &name)),
        None => pad_op(&state, pad::release),
    }
}

#[tauri::command]
async fn pad_combo(state: State<'_, AppState>, buttons: Vec<String>, hold_ms: u64) -> Result<()> {
    let names: Vec<&str> = buttons.iter().map(|s| s.as_str()).collect();
    pad_op(&state, |pad| pad::combo(pad, &names, hold_ms))
}

#[tauri::command]
async fn pad_analog(state: State<'_, AppState>, side: String, direction: String) -> Result<()> {
    pad_op(&state, |pad| pad::analog(pad, &side, &direction))
}

#[tauri::command]
async fn pad_macro(state: State<'_, AppState>, script: String) -> Result<()> {
    pad_op(&state, |pad| pad::macro_run(pad, &script, 120))
}

// ---- payloads and homebrew ----

#[tauri::command]
async fn payload_catalog(state: State<'_, AppState>, refresh: bool) -> Result<Vec<CatalogEntry>> {
    homebrew::catalog(&state.config(), refresh)
}

#[tauri::command]
async fn payload_install(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<u64> {
    let host = state.host()?;
    let config = state.config();
    let entry = homebrew::catalog(&config, false)?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::NotFound(format!("no catalogue entry {id}; refresh the list")))?;
    Ok(spawn_job(
        app,
        state.inner(),
        "payload",
        &entry.name.clone(),
        move |ctx| {
            let report = homebrew::install(&host, &config, &entry, ctx)?;
            ctx.message(report.message);
            Ok(())
        },
    ))
}

#[tauri::command]
async fn payload_send(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<u64> {
    let host = state.host()?;
    let config = state.config();
    let local = PathBuf::from(&path);
    let label = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    Ok(spawn_job(
        app,
        state.inner(),
        "payload",
        &label,
        move |ctx| {
            let report = install::install_local(&host, &local, &config, ctx)?;
            ctx.message(report.message);
            Ok(())
        },
    ))
}

// ---- store and downloads ----

#[tauri::command]
async fn store_search(
    state: State<'_, AppState>,
    query: String,
    source: String,
) -> Result<serde_json::Value> {
    let config = state.config();
    if source == "ia" {
        let items = sources::ia_search(&query, 30)?;
        Ok(serde_json::json!({ "kind": "ia", "items": items }))
    } else {
        let hits = sources::gamesearch(&config.gamesearch_base, &query, 40)?;
        Ok(serde_json::json!({ "kind": "gamesearch", "games": hits }))
    }
}

#[tauri::command]
async fn store_files(state: State<'_, AppState>, item: String) -> Result<Vec<IaFile>> {
    let _ = state;
    sources::ia_item_files(&item)
}

#[tauri::command]
async fn store_download(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    id: String,
    name: Option<String>,
) -> Result<u64> {
    let config = state.config();
    if source == "ia" {
        let files = sources::ia_item_files(&id)?;
        let file = match name {
            Some(wanted) => files
                .into_iter()
                .find(|f| f.name == wanted)
                .ok_or_else(|| Error::NotFound(format!("no file {wanted} in {id}")))?,
            None => files
                .into_iter()
                .next()
                .ok_or_else(|| Error::NotFound(format!("no downloadable file in {id}")))?,
        };
        let label = file.name.clone();
        return Ok(spawn_job(
            app,
            state.inner(),
            "download",
            &label,
            move |ctx| {
                let path = store::download_url(&file.url, &file.name, &config, ctx)?;
                ctx.message(format!("downloaded to {}", path.display()));
                Ok(())
            },
        ));
    }

    let detail = sources::gamesearch_detail(&config.gamesearch_base, &id).map_err(|e| {
        Error::Http(format!(
            "could not load links for {id}: {e}. Set the game-search backend in Settings; the public site hides links behind sign-in."
        ))
    })?;
    let label = detail.title.clone();
    Ok(spawn_job(
        app,
        state.inner(),
        "download",
        &label,
        move |ctx| {
            let path = store::download_hit(&detail, &config, ctx)?;
            ctx.message(format!("downloaded to {}", path.display()));
            Ok(())
        },
    ))
}

#[derive(Serialize)]
struct DownloadsView {
    dir: String,
    files: Vec<fm::FmEntry>,
}

#[tauri::command]
async fn downloads_list(state: State<'_, AppState>) -> Result<DownloadsView> {
    let config = state.config();
    let dir = godsend_core::aria2::download_dir(&config);
    let files = fm::local_list(&dir).unwrap_or_default();
    Ok(DownloadsView {
        dir: dir.display().to_string(),
        files,
    })
}

#[tauri::command]
async fn download_delete(name: String) -> Result<()> {
    let config = Config::load();
    let dir = godsend_core::aria2::download_dir(&config);
    let path = safe_child(&dir, &name)?;
    fm::delete_local(&path)
}

#[tauri::command]
async fn download_open(name: String) -> Result<String> {
    let config = Config::load();
    let dir = godsend_core::aria2::download_dir(&config);
    let path = safe_child(&dir, &name)?;
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    reveal_in_finder(&path)?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn download_push(app: AppHandle, state: State<'_, AppState>, name: String) -> Result<u64> {
    let host = state.host()?;
    let config = state.config();
    let dir = godsend_core::aria2::download_dir(&config);
    let path = safe_child(&dir, &name)?;
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    Ok(spawn_job(
        app,
        state.inner(),
        "install",
        &name,
        move |ctx| {
            let report = install::install_local(&host, &path, &config, ctx)?;
            ctx.message(report.message);
            Ok(())
        },
    ))
}

#[tauri::command]
async fn install_path(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<u64> {
    let host = state.host()?;
    let config = state.config();
    let local = PathBuf::from(&path);
    let label = local
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    Ok(spawn_job(
        app,
        state.inner(),
        "install",
        &label,
        move |ctx| {
            let report = install::install_local(&host, &local, &config, ctx)?;
            ctx.message(report.message);
            Ok(())
        },
    ))
}

// ---- file manager ----

#[tauri::command]
async fn fm_local(path: String) -> Result<Vec<fm::FmEntry>> {
    fm::local_list(&PathBuf::from(path))
}

#[tauri::command]
async fn fm_remote(state: State<'_, AppState>, path: String) -> Result<Vec<fm::FmEntry>> {
    fm::remote_list(&state.host()?, &path)
}

#[tauri::command]
async fn fm_mkdir(path: String, name: String) -> Result<String> {
    let created = fm::mkdir_local(&PathBuf::from(path), &name)?;
    Ok(created.display().to_string())
}

#[tauri::command]
async fn fm_mkdir_remote(state: State<'_, AppState>, path: String) -> Result<()> {
    fm::mkdir_remote(&state.host()?, &path)
}

#[tauri::command]
async fn fm_delete(path: String, is_dir: bool, remote: bool, state: State<'_, AppState>) -> Result<()> {
    if remote {
        fm::delete_remote(&state.host()?, &path, is_dir)
    } else {
        fm::delete_local(&PathBuf::from(path))
    }
}

#[tauri::command]
async fn fm_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    local: String,
    remote_dir: String,
) -> Result<u64> {
    let host = state.host()?;
    let label = local.rsplit('/').next().unwrap_or(&local).to_string();
    Ok(spawn_job(
        app,
        state.inner(),
        "upload",
        &label,
        move |ctx| {
            let config = godsend_core::Config::load();
            let path = fm::upload(
                &host,
                &PathBuf::from(local),
                &remote_dir,
                config.split_bytes,
                ctx,
            )?;
            ctx.message(format!("uploaded to {path}"));
            Ok(())
        },
    ))
}

#[tauri::command]
async fn fm_download(
    app: AppHandle,
    state: State<'_, AppState>,
    remote: String,
    local_dir: String,
) -> Result<u64> {
    let host = state.host()?;
    let label = remote.rsplit('/').next().unwrap_or(&remote).to_string();
    Ok(spawn_job(
        app,
        state.inner(),
        "download",
        &label,
        move |ctx| {
            let path = fm::download(&host, &remote, &PathBuf::from(local_dir), ctx)?;
            ctx.message(format!("saved to {}", path.display()));
            Ok(())
        },
    ))
}

// ---- conversions ----

#[tauri::command]
async fn convert_inspect(path: String) -> Result<convert::ConvertInfo> {
    convert::inspect(&PathBuf::from(path))
}

#[tauri::command]
async fn convert_run(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    src: String,
    out: String,
    chunk_mb: Option<u64>,
) -> Result<u64> {
    let src_path = PathBuf::from(&src);
    let out_path = PathBuf::from(&out);
    let label = kind.clone();
    Ok(spawn_job(
        app,
        state.inner(),
        "convert",
        &label,
        move |ctx| match kind.as_str() {
            "folder_to_iso" => convert::folder_to_iso(&src_path, &out_path, ctx).map(|_| ()),
            "iso_to_folder" => convert::iso_to_folder(&src_path, &out_path, ctx).map(|_| ()),
            "split" => {
                let chunk = chunk_mb.unwrap_or(4095) * 1024 * 1024;
                convert::split_file(&src_path, chunk, ctx).map(|_| ())
            }
            "merge" => convert::merge_file(&src_path, &out_path, ctx).map(|_| ()),
            "folder_to_zip" => convert::folder_to_zip(&src_path, &out_path, ctx).map(|_| ()),
            "zip_to_folder" => convert::zip_to_folder(&src_path, &out_path, ctx).map(|_| ()),
            other => Err(Error::Invalid(format!("unknown conversion {other}"))),
        },
    ))
}

// ---- settings, jobs, misc ----

#[tauri::command]
async fn config_get(state: State<'_, AppState>) -> Result<Config> {
    Ok(state.config())
}

#[tauri::command]
async fn config_set(state: State<'_, AppState>, patch: serde_json::Value) -> Result<Config> {
    let mut config = state.config();
    config.patch_from_json(&patch)?;
    *state.config.lock().expect("config lock") = config.clone();
    Ok(config)
}

#[tauri::command]
async fn doctor_run(state: State<'_, AppState>) -> Result<Vec<doctor::Check>> {
    let config = state.config();
    Ok(doctor::run_with_statuses(&config, &device::probe_all(&config.hosts)))
}

#[tauri::command]
fn jobs(state: State<'_, AppState>) -> Vec<Job> {
    state.jobs.list()
}

#[tauri::command]
fn job_cancel(state: State<'_, AppState>, id: u64) -> bool {
    state.jobs.cancel(id)
}

#[tauri::command]
fn jobs_clear(state: State<'_, AppState>) {
    state.jobs.clear_finished();
}

#[tauri::command]
async fn reveal(path: String) -> Result<()> {
    reveal_in_finder(&PathBuf::from(path))
}

#[tauri::command]
async fn open_data_dir() -> Result<String> {
    let dir = godsend_core::config::data_dir();
    std::fs::create_dir_all(&dir)?;
    reveal_in_finder(&dir)?;
    Ok(dir.display().to_string())
}

fn spawn_job<F>(app: AppHandle, state: &AppState, kind: &str, label: &str, work: F) -> u64
where
    F: FnOnce(&JobContext) -> Result<()> + Send + 'static,
{
    let handle = app.clone();
    state.jobs.spawn(
        kind,
        label,
        move |job| {
            let _ = handle.emit("job://job", job);
        },
        work,
    )
}

fn safe_child(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.trim().is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
    {
        return Err(Error::Invalid(format!("'{name}' is not a file name")));
    }
    Ok(dir.join(name))
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn reveal_in_finder(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    #[cfg(target_os = "macos")]
    let ok = std::process::Command::new("open").arg(path).spawn().is_ok();
    #[cfg(target_os = "windows")]
    let ok = std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .is_ok();
    #[cfg(all(unix, not(target_os = "macos")))]
    let ok = std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .is_ok();
    if ok {
        Ok(())
    } else {
        Err(Error::Other("could not open the file manager".into()))
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "godsend_core=info".into()),
        )
        .init();
    let _ = godsend_core::config::ensure_dirs();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            set_progress_hook(Some(Arc::new(move |job_id, done, total| {
                let _ = handle.emit(
                    "job://progress",
                    serde_json::json!({ "id": job_id, "done": done, "total": total }),
                );
            })));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            probe_host,
            probe_all,
            discover,
            host_add,
            host_remove,
            host_set_active,
            library_scan,
            icon,
            library_delete,
            pad_connect,
            pad_status,
            pad_disconnect,
            pad_press,
            pad_hold,
            pad_release,
            pad_combo,
            pad_analog,
            pad_macro,
            payload_catalog,
            payload_install,
            payload_send,
            store_search,
            store_files,
            store_download,
            install_path,
            downloads_list,
            download_delete,
            download_open,
            download_push,
            fm_local,
            fm_remote,
            fm_mkdir,
            fm_mkdir_remote,
            fm_delete,
            fm_upload,
            fm_download,
            convert_inspect,
            convert_run,
            config_get,
            config_set,
            doctor_run,
            jobs,
            job_cancel,
            jobs_clear,
            reveal,
            open_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GODsend PS5");
}

#[allow(unused)]
fn _keep_types(_: GameEntry, _: GameHit, _: IaItem, _: Job) {}
