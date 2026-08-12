#![forbid(unsafe_code)]

use deltamod_asset_runtime::{
    headers, plan_range, validate_deep_link, AssetRuntime, Body, DeepLinkState, Error, Pending,
    Range, Roots,
};
use http::{Request, Response, StatusCode};
use std::{collections::HashMap, io::Read};
use tauri::{Manager, UriSchemeContext};

const MAIN_LABEL: &str = "main";
#[cfg(any(target_os = "windows", target_os = "android"))]
const MAIN_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(any(target_os = "windows", target_os = "android")))]
const MAIN_ORIGIN: &str = "tauri://localhost";

#[derive(Clone)]
struct AssetState {
    runtime: AssetRuntime,
}

fn error_status(error: Error) -> StatusCode {
    match error {
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::Io => StatusCode::INTERNAL_SERVER_ERROR,
        Error::TooLarge
        | Error::Malformed
        | Error::InvalidScheme
        | Error::InvalidHost
        | Error::InvalidPath
        | Error::NotAllowed => StatusCode::FORBIDDEN,
    }
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-length", "0")
        .body(Vec::new())
        .expect("static response headers are valid")
}

fn origin_allowed(request: &Request<Vec<u8>>) -> bool {
    request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        == Some(MAIN_ORIGIN)
}

fn internal_uri(
    external_scheme: &str,
    internal_scheme: &str,
    uri: &http::Uri,
) -> Result<String, Error> {
    let path = uri.path_and_query().ok_or(Error::Malformed)?.as_str();
    if uri.scheme_str() == Some(external_scheme) {
        let authority = uri.authority().ok_or(Error::Malformed)?;
        return Ok(format!("{internal_scheme}://{authority}{path}"));
    }
    // Windows and Android expose registered schemes as http://<scheme>.localhost/.
    if uri.authority().map(|value| value.as_str()) == Some(&format!("{external_scheme}.localhost"))
    {
        let value = path.strip_prefix('/').ok_or(Error::Malformed)?;
        let (host, asset_path) = value.split_once('/').ok_or(Error::Malformed)?;
        return Ok(format!("{internal_scheme}://{host}/{asset_path}"));
    }
    Err(Error::Malformed)
}

fn serve<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    kind_scheme: &'static str,
) -> Response<Vec<u8>> {
    if ctx.webview_label() != MAIN_LABEL || !origin_allowed(&request) {
        return empty(StatusCode::FORBIDDEN);
    }
    let raw = match internal_uri(kind_scheme, kind_scheme, request.uri()) {
        Ok(raw) => raw,
        Err(error) => return empty(error_status(error)),
    };
    let state = ctx.app_handle().state::<AssetState>();
    let plan = match state.runtime.resolve(&raw) {
        Ok(plan) => plan,
        Err(error) => return empty(error_status(error)),
    };
    let range_header = request.headers().get("range").and_then(|v| v.to_str().ok());
    let response_headers = match headers(&plan, range_header) {
        Ok(value) => value,
        Err(_) => return empty(StatusCode::RANGE_NOT_SATISFIABLE),
    };
    if response_headers.status == StatusCode::RANGE_NOT_SATISFIABLE.as_u16() {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header("content-length", "0")
            .header(
                "content-range",
                response_headers.content_range.unwrap_or_default(),
            )
            .header("accept-ranges", response_headers.accept_ranges)
            .body(Vec::new())
            .expect("static response headers are valid");
    }
    let file = match state.runtime.open(&plan) {
        Ok(file) => file,
        Err(error) => return empty(error_status(error)),
    };
    let range = match plan_range(range_header, plan.length) {
        Ok(range @ (Range::Full | Range::Partial { .. })) => range,
        _ => return empty(StatusCode::RANGE_NOT_SATISFIABLE),
    };
    let mut body = match Body::new(file, range, plan.length) {
        Ok(body) => body,
        Err(error) => return empty(error_status(error)),
    };
    let mut bytes = Vec::with_capacity(response_headers.content_length as usize);
    if body
        .by_ref()
        .take(response_headers.content_length)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return empty(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(response_headers.status).expect("runtime status is valid"))
        .header("content-type", response_headers.content_type)
        .header("content-length", response_headers.content_length)
        .header("accept-ranges", response_headers.accept_ranges)
        .header("access-control-allow-origin", MAIN_ORIGIN);
    if let Some(content_range) = response_headers.content_range {
        builder = builder.header("content-range", content_range);
    }
    builder
        .body(bytes)
        .expect("static response headers are valid")
}

fn queue_urls(state: &DeepLinkState, args: &[String]) {
    for value in args {
        if validate_deep_link(value).is_ok() {
            let _ = state.enqueue(Pending::DeepLink(value.clone()));
        }
    }
}

fn roots_from_environment(app: &tauri::AppHandle) -> Result<Roots, Box<dyn std::error::Error>> {
    let data = app.path().app_data_dir()?;
    let resource = app.path().resource_dir()?;
    let themes = data.join("themes");
    let packets = data.join("packets");
    std::fs::create_dir_all(&themes)?;
    std::fs::create_dir_all(&packets)?;
    Ok(Roots {
        app: resource,
        builtin_theme: themes.clone(),
        user_theme: Some(themes),
        packets: HashMap::new(),
    })
}

fn main() {
    let pending = DeepLinkState::new();
    let pending_for_setup = pending.clone();
    let builder = tauri::Builder::default()
        .manage(pending)
        .register_uri_scheme_protocol("themeprot", |ctx, request| serve(ctx, request, "theme"))
        .register_uri_scheme_protocol("packet", |ctx, request| serve(ctx, request, "packet"));
    #[cfg(feature = "plugins")]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(
            move |_app, argv, _cwd| {
                queue_urls(&pending_for_setup, &argv);
            },
        ))
        .plugin(tauri_plugin_deep_link::init());
    builder
        .setup(move |app| {
            let runtime = AssetRuntime::new(roots_from_environment(app.handle())?)
                .map_err(|_| "asset roots unavailable")?;
            app.manage(AssetState { runtime });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run protocol adapter example");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_errors_without_paths() {
        assert_eq!(error_status(Error::NotFound), StatusCode::NOT_FOUND);
        assert_eq!(error_status(Error::NotAllowed), StatusCode::FORBIDDEN);
        assert_eq!(error_status(Error::Io), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!format!("{}", Error::NotFound).contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn origin_and_window_policy_is_exact() {
        let request = Request::builder()
            .header("origin", MAIN_ORIGIN)
            .body(Vec::new())
            .unwrap();
        assert!(origin_allowed(&request));
        let request = Request::builder()
            .header("origin", "http://evil.localhost")
            .body(Vec::new())
            .unwrap();
        assert!(!origin_allowed(&request));
    }

    #[test]
    fn accepts_native_and_windows_custom_uri_forms() {
        let native: http::Uri = "themeprot://base/theme.json".parse().unwrap();
        assert_eq!(
            internal_uri("themeprot", "theme", &native).unwrap(),
            "theme://base/theme.json"
        );
        let windows: http::Uri = "http://themeprot.localhost/base/theme.json"
            .parse()
            .unwrap();
        assert_eq!(
            internal_uri("themeprot", "theme", &windows).unwrap(),
            "theme://base/theme.json"
        );
    }
}
