//! Tauri 2.11 integration reference. Keep this file in the application crate or include it
//! behind the application's Tauri dependency; the core crate deliberately has no Tauri dep.
#![forbid(unsafe_code)]

use deltamod_asset_runtime::{headers, validate_deep_link, AssetRuntime, Body};
use tauri::Manager;

pub fn register_asset_scheme(app: &tauri::AppHandle, runtime: AssetRuntime) {
    app.register_uri_scheme_protocol("deltamod", move |_app, request| {
        let raw = request.uri().to_string();
        let plan = match runtime.resolve(&raw) {
            Ok(plan) => plan,
            Err(_) => return tauri::ipc::Response::new(Vec::new()),
        };
        let range = request.headers().get("range").and_then(|v| v.to_str().ok());
        let response = match headers(&plan, range) {
            Ok(response) => response,
            Err(_) => return tauri::ipc::Response::new(Vec::new()),
        };
        let file = match runtime.open(&plan) { Ok(file) => file, Err(_) => return tauri::ipc::Response::new(Vec::new()) };
        let body = match Body::new(file, deltamod_asset_runtime::plan_range(range, plan.length).unwrap_or(deltamod_asset_runtime::Range::Unsatisfiable), plan.length) { Ok(body) => body, Err(_) => return tauri::ipc::Response::new(Vec::new()) };
        let mut bytes = Vec::with_capacity(response.content_length as usize);
        let _ = std::io::Read::take(body, response.content_length).read_to_end(&mut bytes);
        http::Response::builder().status(response.status).header("content-type", response.content_type).header("content-length", response.content_length).header("accept-ranges", response.accept_ranges).header("content-range", response.content_range.unwrap_or_default()).body(bytes).unwrap()
    });
}

pub fn single_instance_callback(state: &deltamod_asset_runtime::DeepLinkState, argv: &[String]) {
    for arg in argv { if validate_deep_link(arg).is_ok() { let _ = state.enqueue(deltamod_asset_runtime::Pending::DeepLink(arg.clone())); } }
}

// Application wiring (Tauri plugins remain application policy, not core policy):
// builder.plugin(tauri_plugin_single_instance::init(|_app, argv, _cwd| {
//     single_instance_callback(&state, &argv);
// }));
// builder.plugin(tauri_plugin_deep_link::init());
