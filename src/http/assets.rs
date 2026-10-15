//! The embedded web UI.
//!
//! Built assets are compiled into the binary (and served from disk in debug
//! builds). Hashed files are cached forever; index.html is never cached and
//! gets the URL base injected so the app works behind a path prefix.

use std::borrow::Cow;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::app::App;

/// Built by `npm run build` in web/. A binary built without it still runs
/// (the API works) and explains how to build the UI.
#[derive(RustEmbed)]
#[folder = "web/dist/"]
#[allow_missing = true]
struct Assets;

fn accepts(headers: &HeaderMap, encoding: &str) -> bool {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|e| e.trim().split(';').next() == Some(encoding)))
}

fn load(app: &App, path: &str) -> Option<Cow<'static, [u8]>> {
    if let Some(dir) = &app.boot.web_dir {
        return std::fs::read(dir.join(path)).ok().map(Cow::Owned);
    }
    Assets::get(path).map(|f| f.data)
}

fn index_html(app: &App) -> Response {
    let Some(raw) = load(app, "index.html") else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "The web UI was not built into this binary. Run `npm run build` in web/ and rebuild.",
        )
            .into_response();
    };
    let settings = app.settings.get();
    let base = settings.general.url_base.clone();
    let html = String::from_utf8_lossy(&raw);
    let config = serde_json::json!({
        "base": base,
        "version": crate::VERSION,
        "name": settings.general.instance_name,
    })
    .to_string();
    let escaped = config.replace('&', "&amp;").replace('\'', "&#39;").replace('<', "&lt;");
    let injected = html.replacen(
        "<head>",
        &format!("<head><base href=\"{base}/\"><meta name=\"nodestarr-config\" content='{escaped}'>"),
        1,
    );
    let mut resp = injected.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    resp
}

pub async fn serve(State(app): State<App>, req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.starts_with("api/") {
        return crate::error::AppError::not_found("endpoint").into_response();
    }
    if path.is_empty() || path == "index.html" {
        return index_html(&app);
    }
    // Only plain relative paths reach the file system (NODESTARR_WEB_DIR serves
    // from disk): no "." or ".." segments and no backslashes.
    if path.contains('\\') || path.split('/').any(|seg| seg == "." || seg == "..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let headers = req.headers();
    let (data, encoding) = if accepts(headers, "br")
        && let Some(d) = load(&app, &format!("{path}.br"))
    {
        (Some(d), Some("br"))
    } else if accepts(headers, "gzip")
        && let Some(d) = load(&app, &format!("{path}.gz"))
    {
        (Some(d), Some("gzip"))
    } else {
        (load(&app, path), None)
    };
    let Some(data) = data else {
        // Unknown paths that look like files are 404s; everything else is a
        // client-side route.
        let last = path.rsplit('/').next().unwrap_or("");
        if last.contains('.') && !last.ends_with(".html") {
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
        return index_html(&app);
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut resp = data.into_owned().into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(enc) = encoding {
        h.insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
        h.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    }
    let immutable = path.starts_with("assets/");
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        }),
    );
    resp
}
