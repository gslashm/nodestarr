//! Login, setup, two-factor, sessions, users and API keys.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::activity::NewActivity;
use crate::app::App;
use crate::auth::{AuthVia, LoginResult, Role, SESSION_COOKIE};
use crate::error::{AppError, Result};
use crate::http::{Auth, ClientIp};
use crate::notify::Notification;
use crate::settings::Severity;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/setup", post(setup))
        .route("/auth/login", post(login))
        .route("/auth/mfa", post(mfa))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/password", post(change_password))
        .route("/auth/2fa/setup", post(totp_setup))
        .route("/auth/2fa/enable", post(totp_enable))
        .route("/auth/2fa/disable", post(totp_disable))
        .route("/auth/sessions", get(sessions))
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/users", get(users).post(create_user))
        .route("/users/{id}", delete(delete_user))
        .route("/users/{id}/role", put(set_role))
        .route("/users/{id}/password", put(set_password))
        .route("/apikeys", get(api_keys).post(create_api_key))
        .route("/apikeys/{id}", delete(delete_api_key))
}

fn cookie(app: &App, headers: &HeaderMap, value: &str, max_age: i64) -> HeaderValue {
    let base = app.settings.get().general.url_base.clone();
    let path = if base.is_empty() { "/".to_string() } else { base };
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let s = format!(
        "{SESSION_COOKIE}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    );
    HeaderValue::from_str(&s).unwrap_or(HeaderValue::from_static(""))
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().find_map(|v| {
        v.to_str().ok()?.split(';').find_map(|p| {
            let (k, val) = p.trim().split_once('=')?;
            (k == SESSION_COOKIE).then(|| val.to_string())
        })
    })
}

async fn status(State(app): State<App>, headers: HeaderMap, principal: Option<Auth>) -> Result<Json<Value>> {
    let settings = app.settings.get();
    let setup_required = app.auth.user_count().await? == 0;
    let mut mfa_pending = false;
    if principal.is_none() {
        if let Some(t) = session_token(&headers) {
            if let Ok(Some((_, pending))) = app.auth.session_principal(&t).await {
                mfa_pending = pending;
            }
        }
    }
    Ok(Json(json!({
        "authenticated": principal.is_some(),
        "user": principal.map(|Auth(p)| json!({ "username": p.username, "role": p.role, "via": p.via })),
        "setup_required": setup_required,
        "mfa_pending": mfa_pending,
        "auth_mode": settings.security.auth_mode,
        "instance_name": settings.general.instance_name,
        "version": crate::VERSION,
    })))
}

#[derive(Deserialize)]
struct SetupBody {
    username: String,
    password: String,
    code: String,
}

async fn setup(State(app): State<App>, ClientIp(ip): ClientIp, headers: HeaderMap, Json(b): Json<SetupBody>) -> Result<Response> {
    crate::auth::complete_setup(&app, &b.username, &b.password, &b.code).await?;
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_string);
    let LoginResult::Session { token, max_age, .. } = app.auth.login(&b.username, &b.password, true, ip, ua).await?;
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, cookie(&app, &headers, &token, max_age));
    Ok(resp)
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    #[serde(default)]
    remember: bool,
}

async fn login(State(app): State<App>, ClientIp(ip): ClientIp, headers: HeaderMap, Json(b): Json<LoginBody>) -> Result<Response> {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|s| crate::util::truncate(s, 200));
    match app.auth.login(&b.username, &b.password, b.remember, ip, ua).await {
        Ok(LoginResult::Session { token, max_age, mfa_required }) => {
            if !mfa_required {
                app.note(NewActivity::new("security.login", Severity::Info, format!("{} signed in from {ip}", b.username)).actor(&crate::activity::Actor::user(&b.username)))
                    .await;
            }
            let mut resp = Json(json!({ "mfa_required": mfa_required })).into_response();
            resp.headers_mut().insert(header::SET_COOKIE, cookie(&app, &headers, &token, max_age));
            Ok(resp)
        }
        Err(AppError::RateLimited) => {
            app.notifier.send(Notification::new(
                "security.login_failed",
                Severity::Warning,
                "Repeated failed logins",
                format!("Logins from {ip} are temporarily blocked after too many failed attempts."),
            ));
            app.note(NewActivity::new("security.login_failed", Severity::Warning, format!("Too many failed logins from {ip}"))).await;
            Err(AppError::RateLimited)
        }
        Err(AppError::Unauthorized) => Err(AppError::bad_request("wrong username or password")),
        Err(e) => Err(e),
    }
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

async fn mfa(State(app): State<App>, ClientIp(ip): ClientIp, headers: HeaderMap, Json(b): Json<CodeBody>) -> Result<Json<Value>> {
    let token = session_token(&headers).ok_or(AppError::Unauthorized)?;
    app.auth.complete_mfa(&token, &b.code, ip).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<Response> {
    if let Some(t) = session_token(&headers) {
        app.auth.logout(&t).await?;
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, cookie(&app, &headers, "", 0));
    Ok(resp)
}

async fn me(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    let users = app.auth.users().await?;
    let me = p.user_id.and_then(|id| users.into_iter().find(|u| u.id == id));
    Ok(Json(json!({
        "username": p.username,
        "role": p.role,
        "via": p.via,
        "totp_enabled": me.as_ref().map(|u| u.totp_enabled).unwrap_or(false),
        "can_change_password": p.via == AuthVia::Session,
    })))
}

#[derive(Deserialize)]
struct PasswordChange {
    current: String,
    new: String,
}

async fn change_password(State(app): State<App>, Auth(p): Auth, Json(b): Json<PasswordChange>) -> Result<Json<Value>> {
    let id = p.user_id.ok_or_else(|| AppError::bad_request("only signed-in users can change their password"))?;
    if !app.auth.check_password(id, &b.current).await? {
        return Err(AppError::bad_request("the current password is wrong"));
    }
    app.auth.set_password(id, &b.new, p.session_hash.clone()).await?;
    app.note(NewActivity::new("security.password_changed", Severity::Info, format!("{} changed their password", p.username)).actor(&p.actor())).await;
    Ok(Json(json!({ "ok": true })))
}

async fn totp_setup(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    let id = p.user_id.ok_or_else(|| AppError::bad_request("sign in with a user account first"))?;
    let (secret, uri) = app.auth.totp_begin(id, &p.username).await?;
    Ok(Json(json!({ "secret": secret, "uri": uri })))
}

async fn totp_enable(State(app): State<App>, Auth(p): Auth, Json(b): Json<CodeBody>) -> Result<Json<Value>> {
    let id = p.user_id.ok_or_else(|| AppError::bad_request("sign in with a user account first"))?;
    let codes = app.auth.totp_enable(id, &b.code).await?;
    app.note(NewActivity::new("security.2fa", Severity::Info, format!("{} turned on two-factor authentication", p.username)).actor(&p.actor())).await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(Deserialize)]
struct PasswordConfirm {
    password: String,
}

async fn totp_disable(State(app): State<App>, Auth(p): Auth, Json(b): Json<PasswordConfirm>) -> Result<Json<Value>> {
    let id = p.user_id.ok_or_else(|| AppError::bad_request("sign in with a user account first"))?;
    if !app.auth.check_password(id, &b.password).await? {
        return Err(AppError::bad_request("wrong password"));
    }
    app.auth.totp_disable(id).await?;
    app.note(NewActivity::new("security.2fa", Severity::Warning, format!("{} turned off two-factor authentication", p.username)).actor(&p.actor())).await;
    Ok(Json(json!({ "ok": true })))
}

async fn sessions(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    let Some(id) = p.user_id else { return Ok(Json(json!([]))) };
    let list = app.auth.sessions(id, p.session_hash.as_deref()).await?;
    Ok(Json(serde_json::to_value(list)?))
}

async fn revoke_session(State(app): State<App>, Auth(p): Auth, Path(sid): Path<String>) -> Result<Json<Value>> {
    let id = p.user_id.ok_or(AppError::Unauthorized)?;
    app.auth.revoke_session(id, &sid).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn users(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    p.require_admin()?;
    Ok(Json(serde_json::to_value(app.auth.users().await?)?))
}

#[derive(Deserialize)]
struct NewUser {
    username: String,
    password: String,
    role: Role,
}

async fn create_user(State(app): State<App>, Auth(p): Auth, Json(b): Json<NewUser>) -> Result<Json<Value>> {
    let id = app.auth.create_user(&b.username, &b.password, b.role).await?;
    app.note(NewActivity::new("security.user_created", Severity::Info, format!("User '{}' created ({})", b.username, b.role.as_str())).actor(&p.actor())).await;
    Ok(Json(json!({ "id": id })))
}

async fn delete_user(State(app): State<App>, Auth(p): Auth, Path(id): Path<i64>) -> Result<Json<Value>> {
    if p.user_id == Some(id) {
        return Err(AppError::bad_request("you can't delete your own account"));
    }
    app.auth.delete_user(id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RoleBody {
    role: Role,
}

async fn set_role(State(app): State<App>, Path(id): Path<i64>, Json(b): Json<RoleBody>) -> Result<Json<Value>> {
    app.auth.set_role(id, b.role).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NewPassword {
    password: String,
}

async fn set_password(State(app): State<App>, Auth(p): Auth, Path(id): Path<i64>, Json(b): Json<NewPassword>) -> Result<Json<Value>> {
    app.auth.set_password(id, &b.password, None).await?;
    app.note(NewActivity::new("security.password_changed", Severity::Info, "A user's password was reset").actor(&p.actor())).await;
    Ok(Json(json!({ "ok": true })))
}

async fn api_keys(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    p.require_admin()?;
    Ok(Json(serde_json::to_value(app.auth.api_keys().await?)?))
}

#[derive(Deserialize)]
struct NewKey {
    name: String,
    #[serde(default = "admin")]
    role: Role,
}

fn admin() -> Role {
    Role::Admin
}

async fn create_api_key(State(app): State<App>, Auth(p): Auth, Json(b): Json<NewKey>) -> Result<Json<Value>> {
    let (key, info) = app.auth.create_api_key(&b.name, b.role).await?;
    app.note(NewActivity::new("security.api_key", Severity::Info, format!("API key '{}' created", info.name)).actor(&p.actor())).await;
    Ok(Json(json!({ "key": key, "info": info })))
}

async fn delete_api_key(State(app): State<App>, Auth(p): Auth, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.auth.delete_api_key(id).await?;
    app.note(NewActivity::new("security.api_key", Severity::Info, "API key revoked").actor(&p.actor())).await;
    Ok(Json(json!({ "ok": true })))
}
