#[cfg(target_os = "linux")]
#[path = "linux_webkit.rs"]
mod linux_webkit;
#[path = "local_import.rs"]
mod local_import;

use deltamod_protocol_domain::{
    parse_deep_link, CommunityAction, MAX_ID, MAX_QUEUE_ITEMS, MAX_URI_BYTES,
};
use deltamod_tools_runtime::{
    controller_mode_launch, verify_tool, OwnedProcess, ProcessRegistry, ToolKind,
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    AppHandle, Emitter, Manager, WebviewWindow,
};
use tokio::sync::watch;

const CONTROLLER_FLAG: &str = "-controller";
const CONTROLLER_SHA256: &str = "04ACDBB53C96CD99B01FE53A0297AC06308DDAD14B5253A3AF4F9A319985AA45";
const CONTROLLER_EXIT_MENU_ID: &str = "exit-controller-mode";
const CONTROLLER_EXIT_ACCELERATOR: &str = "F11";
const COMMUNITY_PROTOCOL_PREFIX: &str = "deltamod-community://";
const PROTOCOL_FAILURE_MESSAGE: &str = "The GameBanana one-click request failed.";

pub struct ControllerMode {
    enabled: bool,
    executable: std::path::PathBuf,
    registry: ProcessRegistry,
    process: Mutex<Option<OwnedProcess>>,
}

impl ControllerMode {
    pub fn new(resources: &Path) -> Self {
        let enabled =
            cfg!(target_os = "windows") && std::env::args_os().any(|arg| arg == CONTROLLER_FLAG);
        Self {
            enabled,
            executable: resources.join("tools").join("cmodeutil.exe"),
            registry: ProcessRegistry::default(),
            process: Mutex::new(None),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn start(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut process = self
            .process
            .lock()
            .map_err(|_| "controller mode unavailable")?;
        if process.is_some() {
            return Ok(());
        }
        let tool = verify_tool(
            &self.executable,
            ToolKind::ControllerMode,
            Some(CONTROLLER_SHA256),
        )
        .map_err(|_| "controller mode unavailable")?;
        *process = Some(
            self.registry
                .spawn_silent(&controller_mode_launch(&tool))
                .map_err(|_| "controller mode unavailable")?,
        );
        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(mut process) = self.process.lock() {
            if let Some(process) = process.take() {
                let _ = process.terminate();
            }
        }
    }
}

impl Drop for ControllerMode {
    fn drop(&mut self) {
        self.registry.terminate_all();
    }
}

pub fn install_controller_exit_menu(window: &WebviewWindow) -> Result<(), &'static str> {
    let app = window.app_handle();
    let exit = MenuItem::with_id(
        app,
        CONTROLLER_EXIT_MENU_ID,
        "Exit Controller Mode",
        true,
        Some(CONTROLLER_EXIT_ACCELERATOR),
    )
    .map_err(|_| "controller mode menu unavailable")?;
    let view = Submenu::with_items(app, "View", true, &[&exit])
        .map_err(|_| "controller mode menu unavailable")?;
    let menu = Menu::with_items(app, &[&view]).map_err(|_| "controller mode menu unavailable")?;
    window
        .set_menu(menu)
        .map_err(|_| "controller mode menu unavailable")?;
    Ok(())
}

fn controller_menu_event_allowed(controller_enabled: bool, menu_id: &str) -> bool {
    controller_enabled && menu_id == CONTROLLER_EXIT_MENU_ID
}

pub fn handle_controller_menu_event(app: &AppHandle, menu_id: &str) -> bool {
    let controller_enabled = app
        .try_state::<ControllerMode>()
        .is_some_and(|controller| controller.enabled());
    if !controller_menu_event_allowed(controller_enabled, menu_id) {
        return false;
    }
    let _ = app.emit("leave-controller-mode", leave_controller_mode_payload());
    true
}

pub fn leave_controller_mode_payload() -> Value {
    Value::Null
}

#[derive(Debug)]
struct ProtocolQueueInner {
    app_ready: bool,
    generation: u64,
    page_finished_generation: Option<u64>,
    renderer_subscribed_generation: Option<u64>,
    worker_running: bool,
    next_operation_id: u64,
    active: Option<ActiveProtocolOperation>,
    shutdown: bool,
    pending: VecDeque<CommunityAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProtocolOperationPhase {
    Dequeued,
    Running,
}

#[derive(Debug)]
struct ActiveProtocolOperation {
    operation_id: u64,
    generation: u64,
    phase: ProtocolOperationPhase,
    action: CommunityAction,
    cancellation: watch::Sender<bool>,
}

#[derive(Debug)]
struct ProtocolWork {
    operation_id: u64,
    generation: u64,
    action: CommunityAction,
    cancellation: watch::Receiver<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProtocolCompletion {
    Current,
    Stale,
}

#[derive(Debug)]
pub struct ProtocolLaunchState {
    inner: Mutex<ProtocolQueueInner>,
    workers: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
}

impl Default for ProtocolLaunchState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ProtocolQueueInner {
                app_ready: false,
                generation: 0,
                page_finished_generation: None,
                renderer_subscribed_generation: None,
                worker_running: false,
                next_operation_id: 0,
                active: None,
                shutdown: false,
                pending: VecDeque::new(),
            }),
            workers: Mutex::new(Vec::new()),
        }
    }
}

impl ProtocolLaunchState {
    fn renderer_ready(inner: &ProtocolQueueInner) -> bool {
        inner.page_finished_generation == Some(inner.generation)
            && inner.renderer_subscribed_generation == Some(inner.generation)
    }

    fn start_if_ready(inner: &mut ProtocolQueueInner) -> bool {
        if inner.app_ready
            && Self::renderer_ready(inner)
            && !inner.worker_running
            && !inner.shutdown
            && !inner.pending.is_empty()
        {
            inner.worker_running = true;
            true
        } else {
            false
        }
    }

    fn enqueue(&self, action: CommunityAction) -> Result<bool, ()> {
        let mut inner = self.inner.lock().map_err(|_| ())?;
        if inner.shutdown || inner.pending.len() >= MAX_QUEUE_ITEMS {
            return Err(());
        }
        inner.pending.push_back(action);
        Ok(Self::start_if_ready(&mut inner))
    }

    fn app_ready(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.shutdown {
            return false;
        }
        inner.app_ready = true;
        Self::start_if_ready(&mut inner)
    }

    fn cancel_active(inner: &mut ProtocolQueueInner, requeue_dequeued: bool) {
        if let Some(active) = inner.active.take() {
            let _ = active.cancellation.send(true);
            if requeue_dequeued && active.phase == ProtocolOperationPhase::Dequeued {
                inner.pending.push_front(active.action);
            }
        }
    }

    fn invalidate_renderer(inner: &mut ProtocolQueueInner, requeue_dequeued: bool) {
        inner.generation = inner.generation.wrapping_add(1).max(1);
        inner.page_finished_generation = None;
        inner.renderer_subscribed_generation = None;
        Self::cancel_active(inner, requeue_dequeued);
    }

    fn renderer_loading(&self) -> u64 {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        if !inner.shutdown {
            Self::invalidate_renderer(&mut inner, true);
        }
        inner.generation
    }

    fn renderer_finished(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.shutdown {
            return false;
        }
        inner.page_finished_generation = Some(inner.generation);
        Self::start_if_ready(&mut inner)
    }

    fn renderer_subscription(&self) -> Option<u64> {
        let inner = self.inner.lock().ok()?;
        (!inner.shutdown).then_some(inner.generation)
    }

    fn renderer_subscribed(&self, generation: u64) -> Option<bool> {
        let mut inner = self.inner.lock().ok()?;
        if inner.shutdown || inner.generation != generation {
            return None;
        }
        inner.renderer_subscribed_generation = Some(generation);
        Some(Self::start_if_ready(&mut inner))
    }

    fn renderer_ready_now(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|inner| !inner.shutdown && Self::renderer_ready(&inner))
    }

    fn renderer_unloading(&self, generation: u64) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.shutdown || inner.generation != generation {
            return false;
        }
        Self::invalidate_renderer(&mut inner, true);
        true
    }

    fn next(&self) -> Option<ProtocolWork> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        if !inner.app_ready || !Self::renderer_ready(&inner) || inner.shutdown {
            inner.worker_running = false;
            return None;
        }
        if inner.active.is_some() {
            return None;
        }
        let Some(action) = inner.pending.pop_front() else {
            inner.worker_running = false;
            return None;
        };
        inner.next_operation_id = inner.next_operation_id.wrapping_add(1).max(1);
        let operation_id = inner.next_operation_id;
        let generation = inner.generation;
        let (cancellation, receiver) = watch::channel(false);
        inner.active = Some(ActiveProtocolOperation {
            operation_id,
            generation,
            phase: ProtocolOperationPhase::Dequeued,
            action: action.clone(),
            cancellation,
        });
        Some(ProtocolWork {
            operation_id,
            generation,
            action,
            cancellation: receiver,
        })
    }

    fn begin(&self, operation_id: u64, generation: u64) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.shutdown || inner.generation != generation || !Self::renderer_ready(&inner) {
            return false;
        }
        let Some(active) = inner.active.as_mut() else {
            return false;
        };
        if active.operation_id != operation_id
            || active.generation != generation
            || active.phase != ProtocolOperationPhase::Dequeued
            || *active.cancellation.borrow()
        {
            return false;
        }
        active.phase = ProtocolOperationPhase::Running;
        true
    }

    fn complete(&self, operation_id: u64, generation: u64) -> ProtocolCompletion {
        let Ok(mut inner) = self.inner.lock() else {
            return ProtocolCompletion::Stale;
        };
        let current = inner.active.as_ref().is_some_and(|active| {
            active.operation_id == operation_id
                && active.generation == generation
                && active.phase == ProtocolOperationPhase::Running
        }) && inner.generation == generation
            && Self::renderer_ready(&inner)
            && !inner.shutdown;
        if inner.active.as_ref().is_some_and(|active| {
            active.operation_id == operation_id && active.generation == generation
        }) {
            inner.active = None;
        }
        if current {
            ProtocolCompletion::Current
        } else {
            ProtocolCompletion::Stale
        }
    }

    fn emit_for_generation<F>(&self, generation: u64, emit: F) -> bool
    where
        F: FnOnce(),
    {
        let Ok(inner) = self.inner.lock() else {
            return false;
        };
        if inner.shutdown || inner.generation != generation || !Self::renderer_ready(&inner) {
            return false;
        }
        emit();
        true
    }

    fn own_worker(&self, handle: tauri::async_runtime::JoinHandle<()>) {
        let Ok(inner) = self.inner.lock() else {
            handle.abort();
            return;
        };
        if inner.shutdown {
            handle.abort();
            return;
        }
        let Ok(mut workers) = self.workers.lock() else {
            handle.abort();
            return;
        };
        workers.retain(|worker| !worker.inner().is_finished());
        workers.push(handle);
    }

    fn shutdown(&self) {
        let inner = self.inner.lock();
        if let Ok(mut inner) = inner {
            inner.shutdown = true;
            inner.app_ready = false;
            inner.page_finished_generation = None;
            inner.renderer_subscribed_generation = None;
            inner.pending.clear();
            Self::cancel_active(&mut inner, false);
            if let Ok(workers) = self.workers.lock() {
                for worker in workers.iter() {
                    worker.abort();
                }
            }
        } else if let Ok(workers) = self.workers.lock() {
            for worker in workers.iter() {
                worker.abort();
            }
        }
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.pending.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn active_phase(&self) -> Option<ProtocolOperationPhase> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.active.as_ref().map(|active| active.phase))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum HandoffArgument {
    Protocol(CommunityAction),
    Local(OsString),
    RejectedProtocol,
}

fn looks_like_community_protocol(value: &str) -> bool {
    value
        .get(..COMMUNITY_PROTOCOL_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(COMMUNITY_PROTOCOL_PREFIX))
}

fn classify_handoff_argument(value: OsString) -> HandoffArgument {
    let Some(raw) = value.to_str() else {
        return HandoffArgument::Local(value);
    };
    if !looks_like_community_protocol(raw) {
        return HandoffArgument::Local(value);
    }
    match parse_protocol_action(raw) {
        Ok(action) => HandoffArgument::Protocol(action),
        Err(_) => HandoffArgument::RejectedProtocol,
    }
}

fn parse_protocol_action(raw: &str) -> Result<CommunityAction, ()> {
    if let Ok(action) = parse_deep_link(raw) {
        if let CommunityAction::Import {
            file_id, source, ..
        } = &action
        {
            if !crate::channels::import_download::protocol_source_matches_file_id(source, *file_id)
            {
                return Err(());
            }
        }
        return Ok(action);
    }

    if raw.len() > MAX_URI_BYTES || raw.contains(['?', '#']) || raw.chars().any(char::is_control) {
        return Err(());
    }
    let legacy = raw.strip_prefix("deltamod-community://gb/Mod/").ok_or(())?;
    let (item, source) = legacy.split_once('/').ok_or(())?;
    if item.is_empty() || !item.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    let item_id = item.parse::<u32>().map_err(|_| ())?;
    if item_id == 0 || item_id > MAX_ID {
        return Err(());
    }
    let file_id = crate::channels::import_download::protocol_source_file_id(source).ok_or(())?;
    Ok(CommunityAction::Import {
        item_id,
        file_id,
        source: source.to_owned(),
    })
}

fn queue_protocol_action(app: &AppHandle, action: CommunityAction) {
    let Some(state) = app.try_state::<ProtocolLaunchState>() else {
        return;
    };
    let launch_item_id = match &action {
        CommunityAction::Launch { item_id } => Some(*item_id),
        CommunityAction::Import { .. } => None,
    };
    match state.enqueue(action) {
        Ok(start_worker) => {
            if let Some(item_id) = launch_item_id {
                let _ = crate::write_protocol_queue_smoke_evidence(
                    app,
                    item_id,
                    state.renderer_ready_now(),
                );
            }
            if start_worker {
                start_protocol_worker(app.clone());
            }
        }
        Err(()) => emit_protocol_failure(app),
    }
}

#[cfg(target_os = "macos")]
fn queue_protocol_url(app: &AppHandle, raw: &str) {
    if let Ok(action) = parse_protocol_action(raw) {
        queue_protocol_action(app, action);
    }
}

fn route_handoff_arguments<I>(app: &AppHandle, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    let mut local = Vec::new();
    for arg in args {
        match classify_handoff_argument(arg) {
            HandoffArgument::Protocol(action) => queue_protocol_action(app, action),
            HandoffArgument::Local(arg) => local.push(arg),
            HandoffArgument::RejectedProtocol => {}
        }
    }
    local_import::handle_args(app, local);
}

/// Accept handoffs only from Tauri's single-instance plugin. The first argv
/// item is the executable path; it is never interpreted as an import. Every
/// remaining protocol argument still crosses `parse_protocol_action`, and local
/// files still cross the existing extension/path validation boundary.
pub fn protocol_second_instance(app: &AppHandle, args: Vec<String>) {
    local_import::focus_main(app);
    route_handoff_arguments(app, args.into_iter().skip(1).map(OsString::from));
}

fn start_protocol_worker(app: AppHandle) {
    let worker_app = app.clone();
    let handle = tauri::async_runtime::spawn_blocking(move || run_protocol_worker(worker_app));
    if let Some(state) = app.try_state::<ProtocolLaunchState>() {
        state.own_worker(handle);
    } else {
        handle.abort();
    }
}

fn run_protocol_worker(app: AppHandle) {
    loop {
        let work = {
            let Some(state) = app.try_state::<ProtocolLaunchState>() else {
                return;
            };
            state.next()
        };
        let Some(ProtocolWork {
            operation_id,
            generation,
            action,
            cancellation,
        }) = work
        else {
            return;
        };
        let Some(protocol_state) = app.try_state::<ProtocolLaunchState>() else {
            return;
        };
        if !protocol_state.begin(operation_id, generation) {
            continue;
        }
        match action {
            CommunityAction::Import {
                item_id,
                file_id,
                source,
            } => {
                local_import::focus_main(&app);
                protocol_state.emit_for_generation(generation, || {
                    let _ = app.emit("page", "goc-dl");
                });
                let result = app
                    .try_state::<crate::state::AppState>()
                    .ok_or(())
                    .and_then(|state| {
                        let dialogs =
                            deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(
                                &app,
                            );
                        let with_current_generation = |emit: &mut dyn FnMut()| {
                            protocol_state.emit_for_generation(generation, emit);
                        };
                        crate::channels::import_download::run_protocol_import(
                            &app,
                            &state,
                            &dialogs,
                            crate::channels::import_download::ProtocolImportRequest {
                                item_id,
                                file_id,
                                source_url: &source,
                            },
                            &cancellation,
                            &with_current_generation,
                        )
                        .map_err(|_| ())
                    });
                let completion = protocol_state.complete(operation_id, generation);
                if completion == ProtocolCompletion::Current {
                    protocol_state.emit_for_generation(generation, || {
                        if result == Ok(json!(true)) {
                            let _ = app.emit("refresh", Value::Null);
                        } else if result.is_err() {
                            emit_protocol_failure(&app);
                            let _ = app.emit("page", "main");
                        }
                    });
                }
            }
            CommunityAction::Launch { item_id } => {
                local_import::focus_main(&app);
                if protocol_state.complete(operation_id, generation) == ProtocolCompletion::Current
                {
                    if let Err(code) = crate::write_protocol_smoke_evidence(
                        &app,
                        item_id,
                        operation_id,
                        generation,
                    ) {
                        eprintln!("[protocol-smoke] {code}");
                    }
                }
            }
        }
    }
}

fn emit_protocol_failure(app: &AppHandle) {
    let _ = app.emit(
        "gplog",
        json!({"log": PROTOCOL_FAILURE_MESSAGE, "percent": -1.0}),
    );
}

pub fn protocol_app_ready(app: &AppHandle) {
    if app
        .try_state::<ProtocolLaunchState>()
        .is_some_and(|state| state.app_ready())
    {
        start_protocol_worker(app.clone());
    }
}

pub fn protocol_renderer_loading(app: &AppHandle) {
    if let Some(state) = app.try_state::<ProtocolLaunchState>() {
        state.renderer_loading();
    }
}

pub fn protocol_renderer_finished(app: &AppHandle) {
    if app
        .try_state::<ProtocolLaunchState>()
        .is_some_and(|state| state.renderer_finished())
    {
        start_protocol_worker(app.clone());
    }
}

pub fn protocol_renderer_handshake(app: &AppHandle, data: &[Value]) -> Result<Value, String> {
    const INVALID: &str = "TAURI_INVALID_PAYLOAD:protocol:rendererReady";
    let Some(state) = app.try_state::<ProtocolLaunchState>() else {
        return Err("TAURI_INTERNAL_ERROR".into());
    };
    match data {
        [Value::String(phase)] if phase == "subscribe" => state
            .renderer_subscription()
            .map(|generation| json!(generation))
            .ok_or_else(|| "TAURI_INTERNAL_ERROR".into()),
        [Value::String(phase), Value::Number(generation)] if phase == "ready" => {
            let generation = generation.as_u64().ok_or_else(|| INVALID.to_owned())?;
            let Some(start_worker) = state.renderer_subscribed(generation) else {
                return Ok(json!(false));
            };
            if start_worker {
                start_protocol_worker(app.clone());
            }
            Ok(json!(true))
        }
        [Value::String(phase), Value::Number(generation)] if phase == "unloading" => {
            let generation = generation.as_u64().ok_or_else(|| INVALID.to_owned())?;
            Ok(json!(state.renderer_unloading(generation)))
        }
        _ => Err(INVALID.into()),
    }
}

pub fn protocol_shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<ProtocolLaunchState>() {
        state.shutdown();
    }
}

/// Consume OS file-association handoffs without widening the renderer API.
pub fn install_protocols(app: &AppHandle) -> Result<(), &'static str> {
    // Tauri runs setup before config-defined windows are created. Configure the
    // Linux WebKit/EGL environment here so WebKitWebProcess inherits it, and
    // ensure the configured deep-link scheme has a user-level desktop handler.
    #[cfg(target_os = "linux")]
    {
        use tauri_plugin_deep_link::DeepLinkExt;

        linux_webkit::configure();
        app.deep_link()
            .register_all()
            .map_err(|_| "desktop protocol registration unavailable")?;
    }

    #[cfg(target_os = "macos")]
    {
        let plugin = tauri::plugin::Builder::<tauri::Wry, ()>::new("file-handoff-events")
            .on_event(|app, event| {
                if let tauri::RunEvent::Opened { urls } = event {
                    let mut files = Vec::new();
                    for url in urls {
                        if url.scheme() == "deltamod-community" {
                            queue_protocol_url(app, url.as_str());
                        } else if let Ok(path) = url.to_file_path() {
                            files.push(path.into_os_string());
                        }
                    }
                    local_import::handle_args(app, files);
                }
            })
            .build();
        app.plugin(plugin)
            .map_err(|_| "file handoff event bridge unavailable")?;
    }

    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(());
    }
    let startup_app = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        // initialize_with_app runs immediately before main.rs manages AppState.
        // Wait for that narrow boundary before an import can touch application state.
        for _ in 0..3_000 {
            if startup_app.try_state::<crate::state::AppState>().is_some() {
                route_handoff_arguments(&startup_app, args);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let Some(protocol_state) = app.try_state::<ProtocolLaunchState>() else {
        handle.abort();
        return Err("protocol launch state unavailable");
    };
    protocol_state.own_worker(handle);
    Ok(())
}

fn is_protocol_url(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.starts_with("deltamod://") || value.starts_with("deltamod-community://")
}

fn relaunch_args(args: impl IntoIterator<Item = OsString>, controller: bool) -> Vec<OsString> {
    let mut filtered: Vec<_> = args
        .into_iter()
        .filter(|arg| arg != CONTROLLER_FLAG)
        .filter(|arg| arg.to_str().is_none_or(|arg| !is_protocol_url(arg)))
        .collect();
    if controller {
        filtered.push(CONTROLLER_FLAG.into());
    }
    filtered
}

pub fn relaunch(app: &AppHandle, controller: bool) -> Result<(), String> {
    if !cfg!(target_os = "windows") {
        return Err("TAURI_COMMAND_UNAVAILABLE:controller-mode".into());
    }
    let executable = std::env::current_exe().map_err(|_| "controller mode unavailable")?;
    let args = relaunch_args(std::env::args_os().skip(1), controller);
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "controller mode unavailable")?;
    protocol_shutdown(app);
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol_import(item_id: u32) -> CommunityAction {
        CommunityAction::Import {
            item_id,
            file_id: item_id + 1,
            source: format!("https://gamebanana.com/mmdl/{}", item_id + 1),
        }
    }

    #[test]
    fn controller_exit_action_requires_both_controller_mode_and_the_real_menu_item() {
        for (enabled, menu_id, expected) in [
            (false, CONTROLLER_EXIT_MENU_ID, false),
            (true, "ordinary-f11", false),
            (true, CONTROLLER_EXIT_MENU_ID, true),
        ] {
            assert_eq!(controller_menu_event_allowed(enabled, menu_id), expected);
        }
    }

    #[test]
    fn controller_exit_event_preserves_the_null_compatible_no_argument_payload() {
        assert_eq!(leave_controller_mode_payload(), Value::Null);
    }

    #[test]
    fn strict_protocol_handoffs_are_separated_from_local_file_arguments() {
        assert!(matches!(
            classify_handoff_argument(
                "deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com%2Fmmdl%2F13".into()
            ),
            HandoffArgument::Protocol(CommunityAction::Import {
                item_id: 12,
                file_id: 13,
                ..
            })
        ));
        assert_eq!(
            classify_handoff_argument(
                "deltamod-community://gb/Mod/123/https://gamebanana.com/mmdl/456".into()
            ),
            HandoffArgument::Protocol(CommunityAction::Import {
                item_id: 123,
                file_id: 456,
                source: "https://gamebanana.com/mmdl/456".into(),
            })
        );
        assert_eq!(
            classify_handoff_argument("C:\\mods\\safe.modarchive".into()),
            HandoffArgument::Local("C:\\mods\\safe.modarchive".into())
        );
    }

    #[test]
    fn malformed_or_unsafe_deep_links_fail_closed() {
        for raw in [
            "deltamod-community://gb/import?item=12&file=13&source=http%3A%2F%2Fgamebanana.com%2Fmmdl%2F13",
            "deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com.evil.example%2F13",
            "deltamod-community://gb/import?item=12&item=13&file=14&source=https%3A%2F%2Fgamebanana.com%2F14",
            "deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com%2Fmmdl%2F14",
            "deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com%2Fmods%2F13",
            "DELTAMOD-COMMUNITY://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com%2Fmmdl%2F13",
            "deltamod-community://gb/Mod/12/http://gamebanana.com/mmdl/13",
            "deltamod-community://gb/Mod/12/https://gamebanana.com/dl/13",
            "deltamod-community://gb/Mod/12/https://gamebanana.com/mmdl/13/extra",
            "deltamod-community://gb/Mod/12/https://gamebanana.com/mmdl/13?download=1",
            "deltamod-community://gb/Sound/12/https://gamebanana.com/mmdl/13",
        ] {
            assert_eq!(
                classify_handoff_argument(raw.into()),
                HandoffArgument::RejectedProtocol
            );
        }
    }

    #[test]
    fn protocol_queue_waits_for_both_app_and_renderer_then_runs_serially() {
        let state = ProtocolLaunchState::default();
        assert!(!state.enqueue(protocol_import(1)).unwrap());
        assert!(!state.enqueue(protocol_import(3)).unwrap());
        assert_eq!(state.pending_len(), 2);
        assert!(!state.app_ready());
        let generation = state.renderer_loading();
        assert_eq!(state.renderer_subscription(), Some(generation));
        assert_eq!(state.renderer_subscribed(generation), Some(false));
        assert!(state.renderer_finished());

        let first = state.next().unwrap();
        assert_eq!(first.action, protocol_import(1));
        assert!(state.begin(first.operation_id, first.generation));
        assert_eq!(
            state.complete(first.operation_id, first.generation),
            ProtocolCompletion::Current
        );
        let second = state.next().unwrap();
        assert_eq!(second.action, protocol_import(3));
        assert!(state.begin(second.operation_id, second.generation));
        assert_eq!(
            state.complete(second.operation_id, second.generation),
            ProtocolCompletion::Current
        );
        assert!(state.next().is_none());
        assert_eq!(state.pending_len(), 0);

        assert!(state.enqueue(protocol_import(5)).unwrap());
        let generation = state.renderer_loading();
        assert!(state.next().is_none());
        assert_eq!(state.pending_len(), 1);
        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(generation), Some(true));
        let latest = state.next().unwrap();
        assert_eq!(latest.action, protocol_import(5));
        assert!(state.begin(latest.operation_id, latest.generation));
        assert_eq!(
            state.complete(latest.operation_id, latest.generation),
            ProtocolCompletion::Current
        );
        assert!(state.next().is_none());
    }

    #[test]
    fn page_finished_without_a_listener_subscription_cannot_start_protocol_work() {
        let state = ProtocolLaunchState::default();
        assert!(!state.enqueue(protocol_import(1)).unwrap());
        assert!(!state.app_ready());
        let generation = state.renderer_loading();

        assert!(!state.renderer_finished());
        assert!(state.next().is_none());
        assert_eq!(state.pending_len(), 1);
        assert_eq!(state.renderer_subscribed(generation), Some(true));
    }

    #[test]
    fn page_started_after_dequeue_cancels_and_requeues_unstarted_work() {
        let state = ProtocolLaunchState::default();
        assert!(!state.enqueue(protocol_import(1)).unwrap());
        assert!(!state.app_ready());
        let first_generation = state.renderer_loading();
        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(first_generation), Some(true));
        let work = state.next().unwrap();
        assert_eq!(state.active_phase(), Some(ProtocolOperationPhase::Dequeued));

        let next_generation = state.renderer_loading();
        assert_ne!(next_generation, first_generation);
        assert!(*work.cancellation.borrow());
        assert!(!state.begin(work.operation_id, work.generation));
        assert_eq!(state.pending_len(), 1);
        assert!(state.next().is_none());
        assert_eq!(
            state.complete(work.operation_id, work.generation),
            ProtocolCompletion::Stale
        );

        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(next_generation), Some(true));
        assert_eq!(state.next().unwrap().action, protocol_import(1));
    }

    #[test]
    fn stale_generation_handshakes_and_running_work_fail_closed() {
        let state = ProtocolLaunchState::default();
        assert!(!state.enqueue(protocol_import(1)).unwrap());
        assert!(!state.app_ready());
        let first_generation = state.renderer_loading();
        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(first_generation), Some(true));
        let work = state.next().unwrap();
        assert!(state.begin(work.operation_id, work.generation));

        let next_generation = state.renderer_loading();
        assert!(*work.cancellation.borrow());
        assert_eq!(state.pending_len(), 0);
        let mut stale_event_emitted = false;
        assert!(!state.emit_for_generation(first_generation, || {
            stale_event_emitted = true;
        }));
        assert!(!stale_event_emitted);
        assert_eq!(state.renderer_subscribed(first_generation), None);
        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(next_generation), Some(false));
        assert_eq!(
            state.complete(work.operation_id, work.generation),
            ProtocolCompletion::Stale
        );
    }

    #[test]
    fn shutdown_cancels_active_work_and_rejects_new_ownership() {
        let state = ProtocolLaunchState::default();
        assert!(!state.enqueue(protocol_import(1)).unwrap());
        assert!(!state.app_ready());
        let generation = state.renderer_loading();
        assert!(!state.renderer_finished());
        assert_eq!(state.renderer_subscribed(generation), Some(true));
        let work = state.next().unwrap();
        assert!(state.begin(work.operation_id, work.generation));

        state.shutdown();
        assert!(*work.cancellation.borrow());
        assert_eq!(state.pending_len(), 0);
        assert!(state.enqueue(protocol_import(3)).is_err());
        assert_eq!(
            state.complete(work.operation_id, work.generation),
            ProtocolCompletion::Stale
        );
    }

    #[test]
    fn protocol_queue_is_bounded() {
        let state = ProtocolLaunchState::default();
        for item_id in 1..=MAX_QUEUE_ITEMS as u32 {
            assert!(!state.enqueue(protocol_import(item_id)).unwrap());
        }
        assert!(state
            .enqueue(protocol_import(MAX_QUEUE_ITEMS as u32 + 1))
            .is_err());
        assert_eq!(state.pending_len(), MAX_QUEUE_ITEMS);
    }

    #[test]
    fn relaunch_arguments_drop_mode_and_protocol_urls() {
        let args = [
            "--developer",
            "-controller",
            "deltamod://install/one",
            "DELTAMOD-COMMUNITY://install/two",
            "--safe",
        ]
        .into_iter()
        .map(OsString::from);
        assert_eq!(
            relaunch_args(args, true),
            vec!["--developer", "--safe", "-controller"]
        );
    }

    #[test]
    fn controller_hash_is_pinned() {
        assert_eq!(CONTROLLER_SHA256.len(), 64);
        assert!(CONTROLLER_SHA256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
    }
}
