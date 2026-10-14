//! Users, sessions, API keys and two-factor authentication.

pub mod password;
pub mod totp;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::db::Db;
use crate::error::{AppError, Result};
use crate::settings::{SettingsStore, Severity};

pub const SESSION_COOKIE: &str = "berth_session";
const LOGIN_WINDOW_SECS: i64 = 900;
const LOGIN_MAX_ATTEMPTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Viewer => "viewer",
        }
    }

    fn parse(s: &str) -> Self {
        if s == "viewer" { Role::Viewer } else { Role::Admin }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthVia {
    Session,
    ApiKey,
    LocalBypass,
    External,
}

/// Who is making a request.
#[derive(Debug, Clone, Serialize)]
pub struct Principal {
    pub user_id: Option<i64>,
    pub username: String,
    pub role: Role,
    pub via: AuthVia,
    #[serde(skip)]
    pub session_hash: Option<String>,
}

impl Principal {
    pub fn actor(&self) -> Actor {
        match self.via {
            AuthVia::ApiKey => Actor::api(&self.username),
            _ => Actor::user(&self.username),
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    pub fn require_admin(&self) -> Result<()> {
        if self.is_admin() {
            Ok(())
        } else {
            Err(AppError::forbidden("this needs an admin account"))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub totp_enabled: bool,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyInfo {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub role: Role,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub current: bool,
}

/// Outcome of a password check.
pub enum LoginResult {
    /// Session token to set as a cookie, its lifetime, and whether a TOTP
    /// code is still required.
    Session { token: String, max_age: i64, mfa_required: bool },
}

#[derive(Clone)]
pub struct AuthService {
    db: Db,
    settings: SettingsStore,
    inner: Arc<Inner>,
}

struct Inner {
    attempts: Mutex<HashMap<IpAddr, Vec<i64>>>,
    setup_code: Mutex<Option<String>>,
    used_steps: Mutex<HashMap<i64, i64>>,
}

impl AuthService {
    pub fn new(db: Db, settings: SettingsStore) -> Self {
        Self {
            db,
            settings,
            inner: Arc::new(Inner {
                attempts: Mutex::new(HashMap::new()),
                setup_code: Mutex::new(None),
                used_steps: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub async fn user_count(&self) -> Result<i64> {
        self.db
            .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?))
            .await
    }

    pub fn setup_code(&self) -> Option<String> {
        self.inner.setup_code.lock().clone()
    }

    /// Rate limiting: records a failure and says whether the IP is blocked.
    fn too_many(&self, ip: IpAddr, record: bool) -> bool {
        let now = crate::util::now();
        let mut map = self.inner.attempts.lock();
        let list = map.entry(ip).or_default();
        list.retain(|t| now - t < LOGIN_WINDOW_SECS);
        if record {
            list.push(now);
        }
        list.len() >= LOGIN_MAX_ATTEMPTS
    }

    fn clear_attempts(&self, ip: IpAddr) {
        self.inner.attempts.lock().remove(&ip);
    }

    pub async fn create_user(&self, username: &str, password: &str, role: Role) -> Result<i64> {
        let username = username.trim().to_string();
        if username.is_empty()
            || username.len() > 64
            || !username.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@'))
        {
            return Err(AppError::bad_request("usernames use letters, digits and _ - . @ (max 64)"));
        }
        password::validate(password).map_err(AppError::BadRequest)?;
        let hash = password::hash(password)?;
        let now = crate::util::now();
        self.db
            .call(move |c| {
                let exists: bool = c
                    .query_row("SELECT 1 FROM users WHERE username = ?1", [&username], |_| Ok(true))
                    .optional()?
                    .unwrap_or(false);
                if exists {
                    return Err(AppError::conflict("that username is taken"));
                }
                c.execute(
                    "INSERT INTO users (username, password_hash, role, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![username, hash, role.as_str(), now],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await
    }

    pub async fn users(&self) -> Result<Vec<UserInfo>> {
        self.db
            .call(|c| {
                let mut stmt = c.prepare(
                    "SELECT id, username, role, totp_enabled, created_at, last_login_at FROM users ORDER BY username",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok(UserInfo {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        role: Role::parse(&r.get::<_, String>(2)?),
                        totp_enabled: r.get::<_, i64>(3)? != 0,
                        created_at: r.get(4)?,
                        last_login_at: r.get(5)?,
                    })
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
    }

    pub async fn delete_user(&self, id: i64) -> Result<()> {
        self.db
            .call(move |c| {
                let admins: i64 = c.query_row(
                    "SELECT COUNT(*) FROM users WHERE role = 'admin' AND id != ?1",
                    [id],
                    |r| r.get(0),
                )?;
                if admins == 0 {
                    return Err(AppError::conflict("can't delete the last admin"));
                }
                c.execute("DELETE FROM users WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    pub async fn set_role(&self, id: i64, role: Role) -> Result<()> {
        self.db
            .call(move |c| {
                if role == Role::Viewer {
                    let others: i64 = c.query_row(
                        "SELECT COUNT(*) FROM users WHERE role = 'admin' AND id != ?1",
                        [id],
                        |r| r.get(0),
                    )?;
                    if others == 0 {
                        return Err(AppError::conflict("at least one admin is required"));
                    }
                }
                c.execute("UPDATE users SET role = ?1 WHERE id = ?2", params![role.as_str(), id])?;
                Ok(())
            })
            .await
    }

    pub async fn set_password(&self, user_id: i64, new_password: &str, keep_session: Option<String>) -> Result<()> {
        password::validate(new_password).map_err(AppError::BadRequest)?;
        let hash = password::hash(new_password)?;
        self.db
            .call(move |c| {
                c.execute(
                    "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
                    params![hash, crate::util::now(), user_id],
                )?;
                // Sign out everywhere else.
                c.execute(
                    "DELETE FROM sessions WHERE user_id = ?1 AND id != ?2",
                    params![user_id, keep_session.unwrap_or_default()],
                )?;
                Ok(())
            })
            .await
    }

    async fn password_hash_of(&self, user_id: i64) -> Result<String> {
        self.db
            .call(move |c| Ok(c.query_row("SELECT password_hash FROM users WHERE id = ?1", [user_id], |r| r.get(0))?))
            .await
    }

    pub async fn check_password(&self, user_id: i64, password: &str) -> Result<bool> {
        let hash = self.password_hash_of(user_id).await?;
        let pw = password.to_string();
        Ok(tokio::task::spawn_blocking(move || password::verify(&pw, &hash)).await?)
    }

    /// Verifies a username and password, creating a session.
    pub async fn login(&self, username: &str, pw: &str, remember: bool, ip: IpAddr, user_agent: Option<String>) -> Result<LoginResult> {
        if self.too_many(ip, false) {
            return Err(AppError::RateLimited);
        }
        let username = username.trim().to_string();
        let row: Option<(i64, String, bool)> = {
            let u = username.clone();
            self.db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT id, password_hash, totp_enabled FROM users WHERE username = ?1",
                        [&u],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
                    )
                    .optional()?)
                })
                .await?
        };
        let pw = pw.to_string();
        let (ok, user) = match row {
            Some((id, hash, totp)) => {
                let ok = tokio::task::spawn_blocking(move || password::verify(&pw, &hash)).await?;
                (ok, Some((id, totp)))
            }
            None => {
                // Same work as a real check, so timing doesn't reveal usernames.
                let _ = tokio::task::spawn_blocking(move || password::verify(&pw, password::dummy_hash())).await;
                (false, None)
            }
        };
        let Some((user_id, totp)) = user.filter(|_| ok) else {
            let blocked = self.too_many(ip, true);
            tracing::warn!(%ip, user = %username, "failed login");
            if blocked {
                return Err(AppError::RateLimited);
            }
            return Err(AppError::Unauthorized);
        };
        self.clear_attempts(ip);
        let settings = self.settings.get();
        let max_age = if remember {
            settings.security.remember_days as i64 * 86400
        } else {
            settings.security.session_hours as i64 * 3600
        };
        let token = crate::util::random_token(32);
        let hash = crate::util::sha256_hex(&token);
        let now = crate::util::now();
        let ip_s = ip.to_string();
        self.db
            .call(move |c| {
                c.execute(
                    "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at, ip, user_agent, mfa_pending)
                     VALUES (?1, ?2, ?3, ?4, ?3, ?5, ?6, ?7)",
                    params![hash, user_id, now, now + max_age, ip_s, user_agent, totp as i64],
                )?;
                if !totp {
                    c.execute("UPDATE users SET last_login_at = ?1 WHERE id = ?2", params![now, user_id])?;
                }
                Ok(())
            })
            .await?;
        Ok(LoginResult::Session { token, max_age, mfa_required: totp })
    }

    /// Completes a login that needs a TOTP or recovery code.
    pub async fn complete_mfa(&self, token: &str, code: &str, ip: IpAddr) -> Result<()> {
        if self.too_many(ip, false) {
            return Err(AppError::RateLimited);
        }
        let hash = crate::util::sha256_hex(token);
        let h2 = hash.clone();
        let row: Option<(i64, Option<String>, Option<String>)> = self
            .db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT u.id, u.totp_secret, u.recovery_codes FROM sessions s JOIN users u ON u.id = s.user_id
                     WHERE s.id = ?1 AND s.mfa_pending = 1 AND s.expires_at > ?2",
                    params![h2, crate::util::now()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?)
            })
            .await?;
        let (user_id, secret, recovery) = row.ok_or(AppError::Unauthorized)?;
        let mut ok = false;
        let mut remaining_recovery: Option<String> = None;
        if let Some(secret) = secret {
            if let Some(step) = totp::verify(&secret, code, crate::util::now()) {
                let mut used = self.inner.used_steps.lock();
                if used.get(&user_id).is_some_and(|s| *s >= step) {
                    return Err(AppError::bad_request("that code was already used; wait for the next one"));
                }
                used.insert(user_id, step);
                ok = true;
            }
        }
        if !ok {
            // Recovery codes: stored hashed, single use.
            let normalized = code.trim().to_ascii_lowercase();
            let code_hash = crate::util::sha256_hex(&normalized);
            let mut list: Vec<String> = recovery.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
            if let Some(pos) = list.iter().position(|h| *h == code_hash) {
                list.remove(pos);
                remaining_recovery = Some(serde_json::to_string(&list)?);
                ok = true;
            }
        }
        if !ok {
            self.too_many(ip, true);
            return Err(AppError::bad_request("that code is not valid"));
        }
        let now = crate::util::now();
        self.db
            .call(move |c| {
                c.execute("UPDATE sessions SET mfa_pending = 0 WHERE id = ?1", [&hash])?;
                c.execute("UPDATE users SET last_login_at = ?1 WHERE id = ?2", params![now, user_id])?;
                if let Some(r) = remaining_recovery {
                    c.execute("UPDATE users SET recovery_codes = ?1 WHERE id = ?2", params![r, user_id])?;
                }
                Ok(())
            })
            .await
    }

    /// Resolves a session cookie.
    pub async fn session_principal(&self, token: &str) -> Result<Option<(Principal, bool)>> {
        let hash = crate::util::sha256_hex(token);
        let now = crate::util::now();
        let h2 = hash.clone();
        let row = self
            .db
            .call(move |c| {
                Ok(c.query_row(
                    "SELECT u.id, u.username, u.role, s.mfa_pending, s.last_seen_at FROM sessions s JOIN users u ON u.id = s.user_id
                     WHERE s.id = ?1 AND s.expires_at > ?2",
                    params![h2, now],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, i64>(3)? != 0,
                            r.get::<_, i64>(4)?,
                        ))
                    },
                )
                .optional()?)
            })
            .await?;
        let Some((id, username, role, mfa_pending, last_seen)) = row else { return Ok(None) };
        if now - last_seen > 300 {
            let h3 = hash.clone();
            let _ = self
                .db
                .call(move |c| {
                    c.execute("UPDATE sessions SET last_seen_at = ?1 WHERE id = ?2", params![now, h3])?;
                    Ok(())
                })
                .await;
        }
        Ok(Some((
            Principal {
                user_id: Some(id),
                username,
                role: Role::parse(&role),
                via: AuthVia::Session,
                session_hash: Some(hash),
            },
            mfa_pending,
        )))
    }

    pub async fn logout(&self, token: &str) -> Result<()> {
        let hash = crate::util::sha256_hex(token);
        self.db
            .call(move |c| {
                c.execute("DELETE FROM sessions WHERE id = ?1", [hash])?;
                Ok(())
            })
            .await
    }

    pub async fn sessions(&self, user_id: i64, current: Option<&str>) -> Result<Vec<SessionInfo>> {
        let current = current.map(str::to_string);
        self.db
            .call(move |c| {
                let mut stmt = c.prepare(
                    "SELECT id, created_at, last_seen_at, expires_at, ip, user_agent FROM sessions
                     WHERE user_id = ?1 AND mfa_pending = 0 AND expires_at > ?2 ORDER BY last_seen_at DESC",
                )?;
                let rows = stmt.query_map(params![user_id, crate::util::now()], |r| {
                    let id: String = r.get(0)?;
                    Ok(SessionInfo {
                        current: current.as_deref() == Some(id.as_str()),
                        id: id[..12].to_string(),
                        created_at: r.get(1)?,
                        last_seen_at: r.get(2)?,
                        expires_at: r.get(3)?,
                        ip: r.get(4)?,
                        user_agent: r.get(5)?,
                    })
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
    }

    /// Revokes a session by the short id shown in the UI.
    pub async fn revoke_session(&self, user_id: i64, short_id: &str) -> Result<()> {
        if short_id.len() < 8 || !short_id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::bad_request("invalid session id"));
        }
        let pat = format!("{short_id}%");
        self.db
            .call(move |c| {
                c.execute("DELETE FROM sessions WHERE user_id = ?1 AND id LIKE ?2", params![user_id, pat])?;
                Ok(())
            })
            .await
    }

    // --- Two-factor ----------------------------------------------------------

    /// Starts enrollment: stores a new secret (not yet active).
    pub async fn totp_begin(&self, user_id: i64, username: &str) -> Result<(String, String)> {
        let secret = totp::new_secret();
        let s2 = secret.clone();
        self.db
            .call(move |c| {
                c.execute("UPDATE users SET totp_secret = ?1, totp_enabled = 0 WHERE id = ?2", params![s2, user_id])?;
                Ok(())
            })
            .await?;
        let issuer = self.settings.get().general.instance_name.clone();
        let uri = totp::otpauth_uri(&secret, username, &issuer);
        Ok((secret, uri))
    }

    /// Confirms enrollment with a code; returns one-time recovery codes.
    pub async fn totp_enable(&self, user_id: i64, code: &str) -> Result<Vec<String>> {
        let secret: Option<String> = self
            .db
            .call(move |c| Ok(c.query_row("SELECT totp_secret FROM users WHERE id = ?1", [user_id], |r| r.get(0))?))
            .await?;
        let secret = secret.ok_or_else(|| AppError::bad_request("start two-factor setup first"))?;
        if totp::verify(&secret, code, crate::util::now()).is_none() {
            return Err(AppError::bad_request("that code doesn't match; check your phone's clock"));
        }
        let codes = totp::recovery_codes();
        let hashes: Vec<String> = codes.iter().map(crate::util::sha256_hex).collect();
        let json = serde_json::to_string(&hashes)?;
        self.db
            .call(move |c| {
                c.execute("UPDATE users SET totp_enabled = 1, recovery_codes = ?1 WHERE id = ?2", params![json, user_id])?;
                Ok(())
            })
            .await?;
        Ok(codes)
    }

    pub async fn totp_disable(&self, user_id: i64) -> Result<()> {
        self.db
            .call(move |c| {
                c.execute(
                    "UPDATE users SET totp_enabled = 0, totp_secret = NULL, recovery_codes = NULL WHERE id = ?1",
                    [user_id],
                )?;
                Ok(())
            })
            .await
    }

    // --- API keys --------------------------------------------------------------

    pub async fn create_api_key(&self, name: &str, role: Role) -> Result<(String, ApiKeyInfo)> {
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > 64 {
            return Err(AppError::bad_request("give the key a name (max 64 characters)"));
        }
        let key = format!("brt_{}", crate::util::random_token(30));
        let hash = crate::util::sha256_hex(&key);
        let prefix = key[..10].to_string();
        let now = crate::util::now();
        let (n2, p2) = (name.clone(), prefix.clone());
        let id = self
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO api_keys (name, key_hash, prefix, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![n2, hash, p2, role.as_str(), now],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await?;
        Ok((
            key,
            ApiKeyInfo {
                id,
                name,
                prefix,
                role,
                created_at: now,
                last_used_at: None,
            },
        ))
    }

    pub async fn api_keys(&self) -> Result<Vec<ApiKeyInfo>> {
        self.db
            .call(|c| {
                let mut stmt = c.prepare("SELECT id, name, prefix, role, created_at, last_used_at FROM api_keys ORDER BY created_at")?;
                let rows = stmt.query_map([], |r| {
                    Ok(ApiKeyInfo {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        prefix: r.get(2)?,
                        role: Role::parse(&r.get::<_, String>(3)?),
                        created_at: r.get(4)?,
                        last_used_at: r.get(5)?,
                    })
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<()> {
        self.db
            .call(move |c| {
                c.execute("DELETE FROM api_keys WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    pub async fn api_key_principal(&self, key: &str) -> Result<Option<Principal>> {
        if !key.starts_with("brt_") {
            return Ok(None);
        }
        let hash = crate::util::sha256_hex(key);
        let now = crate::util::now();
        self.db
            .call(move |c| {
                let row: Option<(i64, String, String, Option<i64>)> = c
                    .query_row(
                        "SELECT id, name, role, last_used_at FROM api_keys WHERE key_hash = ?1",
                        [&hash],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?;
                let Some((id, name, role, last)) = row else { return Ok(None) };
                if last.is_none_or(|l| now - l > 300) {
                    c.execute("UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2", params![now, id])?;
                }
                Ok(Some(Principal {
                    user_id: None,
                    username: name,
                    role: Role::parse(&role),
                    via: AuthVia::ApiKey,
                    session_hash: None,
                }))
            })
            .await
    }
}

/// On first start, creates the admin from env vars or prints a setup code.
pub async fn bootstrap_admin(app: &App) -> Result<()> {
    if app.auth.user_count().await? > 0 {
        return Ok(());
    }
    if let (Some(user), Some(pass)) = (&app.boot.admin_user, &app.boot.admin_password) {
        app.auth.create_user(user, pass, Role::Admin).await?;
        tracing::info!(user = %user, "created admin user from BERTH_ADMIN_USER");
        app.note(NewActivity::new("security.user_created", Severity::Info, format!("Admin '{user}' created from environment")).actor(&Actor::system()))
            .await;
        return Ok(());
    }
    let raw = crate::util::random_token(6).to_ascii_uppercase().replace(['-', '_'], "X");
    let code = format!("{}-{}", &raw[..4], &raw[4..8]);
    *app.auth.inner.setup_code.lock() = Some(code.clone());
    tracing::warn!("==============================================================");
    tracing::warn!("  No users yet. Open Berth in your browser to create one.");
    tracing::warn!("  Setup code: {code}");
    tracing::warn!("==============================================================");
    Ok(())
}

/// Consumes the setup code and creates the first admin.
pub async fn complete_setup(app: &App, username: &str, password: &str, code: &str) -> Result<()> {
    if app.auth.user_count().await? > 0 {
        return Err(AppError::conflict("setup was already completed"));
    }
    let expected = app.auth.setup_code().ok_or_else(|| AppError::conflict("no setup in progress; restart Berth"))?;
    let normalize = |s: &str| s.trim().to_ascii_uppercase().replace(' ', "");
    if !crate::util::ct_eq(normalize(code).as_bytes(), normalize(&expected).as_bytes()) {
        return Err(AppError::bad_request("the setup code is wrong (find it in Berth's logs)"));
    }
    app.auth.create_user(username, password, Role::Admin).await?;
    *app.auth.inner.setup_code.lock() = None;
    app.note(NewActivity::new("security.user_created", Severity::Info, format!("Admin '{username}' created during setup")).actor(&Actor::system()))
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn svc() -> AuthService {
        let db = Db::open_in_memory().unwrap();
        let settings = SettingsStore::load(db.clone()).await.unwrap();
        AuthService::new(db, settings)
    }

    #[tokio::test]
    async fn login_flow_and_rate_limit() {
        let auth = svc().await;
        auth.create_user("jake", "correct horse", Role::Admin).await.unwrap();
        assert!(auth.create_user("JAKE", "another one", Role::Admin).await.is_err(), "usernames are case-insensitive");
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        let LoginResult::Session { token, mfa_required, .. } = auth.login("jake", "correct horse", false, ip, None).await.unwrap();
        assert!(!mfa_required);
        let (p, pending) = auth.session_principal(&token).await.unwrap().unwrap();
        assert_eq!(p.username, "jake");
        assert!(!pending);
        auth.logout(&token).await.unwrap();
        assert!(auth.session_principal(&token).await.unwrap().is_none());

        let bad: IpAddr = "10.0.0.9".parse().unwrap();
        for _ in 0..LOGIN_MAX_ATTEMPTS - 1 {
            assert!(matches!(auth.login("jake", "nope", false, bad, None).await, Err(AppError::Unauthorized)));
        }
        assert!(matches!(auth.login("jake", "nope", false, bad, None).await, Err(AppError::RateLimited)));
        assert!(matches!(auth.login("jake", "correct horse", false, bad, None).await, Err(AppError::RateLimited)));
    }

    #[tokio::test]
    async fn two_factor_and_recovery() {
        let auth = svc().await;
        let id = auth.create_user("jake", "correct horse", Role::Admin).await.unwrap();
        let (secret, _) = auth.totp_begin(id, "jake").await.unwrap();
        let code = super::totp::code_for(&secret, crate::util::now()).unwrap();
        let recovery = auth.totp_enable(id, &code).await.unwrap();
        assert_eq!(recovery.len(), 10);

        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        let LoginResult::Session { token, mfa_required, .. } = auth.login("jake", "correct horse", true, ip, None).await.unwrap();
        assert!(mfa_required);
        let (_, pending) = auth.session_principal(&token).await.unwrap().unwrap();
        assert!(pending);
        auth.complete_mfa(&token, &recovery[0], ip).await.unwrap();
        let (_, pending) = auth.session_principal(&token).await.unwrap().unwrap();
        assert!(!pending);

        // Recovery codes are single use.
        let LoginResult::Session { token: t2, .. } = auth.login("jake", "correct horse", true, ip, None).await.unwrap();
        assert!(auth.complete_mfa(&t2, &recovery[0], ip).await.is_err());
    }

    #[tokio::test]
    async fn api_keys() {
        let auth = svc().await;
        let (key, info) = auth.create_api_key("homeassistant", Role::Viewer).await.unwrap();
        assert!(key.starts_with("brt_"));
        assert_eq!(info.prefix.len(), 10);
        let p = auth.api_key_principal(&key).await.unwrap().unwrap();
        assert_eq!(p.role, Role::Viewer);
        assert_eq!(p.via, AuthVia::ApiKey);
        assert!(auth.api_key_principal("brt_wrong").await.unwrap().is_none());
        auth.delete_api_key(info.id).await.unwrap();
        assert!(auth.api_key_principal(&key).await.unwrap().is_none());
    }
}
