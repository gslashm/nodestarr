//! Request middleware: URL base handling, security headers, authentication
//! and authorization, CSRF protection.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::app::App;
use crate::auth::{AuthVia, Principal, Role, SESSION_COOKIE};
use crate::error::AppError;
use crate::settings::AuthMode;
use crate::util::net::{CidrList, client_ip};

/// The real client address (after trusted proxies).
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

/// Extractor for the authenticated principal.
pub struct Auth(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Auth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Principal>().cloned().map(Auth).ok_or(AppError::Unauthorized)
    }
}

impl<S: Send + Sync> axum::extract::OptionalFromRequestParts<S> for Auth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<Principal>().cloned().map(Auth))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<ClientIp>().copied().unwrap_or(ClientIp(IpAddr::from([127, 0, 0, 1]))))
    }
}

/// Serves Nodestarr under a URL base ("/nodestarr") by stripping it before routing.
pub async fn strip_base(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let base = app.settings.get().general.url_base.clone();
    // The health probe stays reachable without the base, so the container
    // health check works whatever the URL base is.
    if base.is_empty() || req.uri().path() == "/api/v1/health" {
        return next.run(req).await;
    }
    let path = req.uri().path().to_string();
    if path == base {
        return Redirect::permanent(&format!("{base}/")).into_response();
    }
    match path.strip_prefix(&base) {
        Some(rest) if rest.starts_with('/') => {
            let pq = match req.uri().query() {
                Some(q) => format!("{rest}?{q}"),
                None => rest.to_string(),
            };
            if let Ok(uri) = pq.parse::<Uri>() {
                *req.uri_mut() = uri;
            }
            next.run(req).await
        }
        _ if path == "/" => Redirect::temporary(&format!("{base}/")).into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub async fn security_headers(req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    h.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
    h.insert("Referrer-Policy", HeaderValue::from_static("same-origin"));
    if is_api {
        h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    } else {
        h.insert(
            "Content-Security-Policy",
            HeaderValue::from_static(
                "default-src 'self'; img-src 'self' data: blob: https: http:; style-src 'self' 'unsafe-inline'; \
                 script-src 'self'; connect-src 'self' ws: wss:; font-src 'self' data:; frame-ancestors 'self'; \
                 base-uri 'self'; form-action 'self'",
            ),
        );
    }
    resp
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get_all(header::COOKIE).iter().find_map(|v| {
        v.to_str().ok()?.split(';').find_map(|pair| {
            let (k, val) = pair.trim().split_once('=')?;
            (k == name).then(|| val.to_string())
        })
    })
}

fn query_param(uri: &Uri, name: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
    })
}

/// Paths reachable without logging in.
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/health"
            | "/api/v1/auth/status"
            | "/api/v1/auth/login"
            | "/api/v1/auth/mfa"
            | "/api/v1/auth/setup"
            | "/api/v1/auth/logout"
    ) || !(path.starts_with("/api/") || path == "/metrics")
}

/// Mutations a viewer (or a session waiting for its TOTP code) may perform.
fn allowed_for_viewer(path: &str) -> bool {
    path.starts_with("/api/v1/auth/")
}

/// Resolves the principal for a request, if any. Takes the headers and URI
/// rather than the request, whose body isn't `Sync` (so a reference to it
/// can't be held across an await in a `Send` future).
async fn resolve(app: &App, headers: &HeaderMap, uri: &Uri, ip: IpAddr, peer: IpAddr) -> Option<(Principal, bool)> {
    let settings = app.settings.get();
    let sec = &settings.security;

    // 1. Authenticating reverse proxy.
    if sec.auth_mode == AuthMode::External {
        let trusted = CidrList::parse_lossy(&sec.trusted_proxies);
        if trusted.contains(peer) {
            if let Some(user) =
                headers.get(sec.external_header.as_str()).and_then(|v| v.to_str().ok()).filter(|u| !u.is_empty())
            {
                return Some((
                    Principal {
                        user_id: None,
                        username: user.to_string(),
                        role: Role::Admin,
                        via: AuthVia::External,
                        session_hash: None,
                        key_id: None,
                    },
                    false,
                ));
            }
        }
    }

    // 2. API key (header, or query when allowed).
    let key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::to_string)
        })
        .or_else(|| if sec.allow_query_api_key { query_param(uri, "apikey") } else { None });
    if let Some(k) = key {
        if let Ok(Some(p)) = app.auth.api_key_principal(&k).await {
            return Some((p, false));
        }
    }

    // 3. Session cookie.
    if let Some(token) = cookie_value(headers, SESSION_COOKIE) {
        if let Ok(Some((p, mfa_pending))) = app.auth.session_principal(&token).await {
            return Some((p, mfa_pending));
        }
    }

    // 4. Local network bypass.
    if sec.bypass_local && CidrList::parse_lossy(&sec.local_networks).contains(ip) {
        return Some((
            Principal {
                user_id: None,
                username: "local".into(),
                role: Role::Admin,
                via: AuthVia::LocalBypass,
                session_hash: None,
                key_id: None,
            },
            false,
        ));
    }
    None
}

pub async fn authenticate(
    State(app): State<App>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let settings = app.settings.get();
    let trusted = CidrList::parse_lossy(&settings.security.trusted_proxies);
    let xff = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(str::to_string);
    let real = req.headers().get("x-real-ip").and_then(|v| v.to_str().ok()).map(str::to_string);
    let ip = client_ip(addr, xff.as_deref(), real.as_deref(), &trusted);
    req.extensions_mut().insert(ClientIp(ip));

    let path = req.uri().path().to_string();
    let resolved = resolve(&app, req.headers(), req.uri(), ip, crate::util::net::canonical(addr.ip())).await;
    let mut mfa_pending = false;
    if let Some((p, pending)) = &resolved {
        mfa_pending = *pending;
        if !pending {
            req.extensions_mut().insert(p.clone());
        }
    }

    if path == "/metrics" && settings.security.metrics_public {
        return next.run(req).await;
    }
    if is_public(&path) {
        return next.run(req).await;
    }
    let Some((principal, _)) = resolved.filter(|_| !mfa_pending) else {
        return AppError::Unauthorized.into_response();
    };

    let method = req.method().clone();
    let mutating = !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
    if mutating && principal.via == AuthVia::Session {
        // Cross-site requests can't set custom headers without a CORS
        // preflight, which Nodestarr never approves.
        if req.headers().get("x-nodestarr-request").is_none() {
            return AppError::forbidden("missing X-Nodestarr-Request header").into_response();
        }
    }
    if mutating && !principal.is_admin() && !allowed_for_viewer(&path) {
        return AppError::forbidden("this needs an admin account").into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_paths() {
        assert!(is_public("/api/v1/health"));
        assert!(is_public("/"));
        assert!(is_public("/containers/sonarr"));
        assert!(is_public("/assets/index-abc.js"));
        assert!(!is_public("/api/v1/containers"));
        assert!(!is_public("/metrics"));
    }

    #[test]
    fn query_params() {
        let uri: Uri = "/x?a=1&apikey=nsr_abc%2B1".parse().unwrap();
        assert_eq!(query_param(&uri, "apikey").as_deref(), Some("nsr_abc+1"));
        assert_eq!(query_param(&uri, "b"), None);
    }
}
