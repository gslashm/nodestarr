//! Maintenance subcommands, meant for `docker exec nodestarr nodestarr <command>`.

use crate::config::BootConfig;
use crate::db::Db;

fn open_db(boot: &BootConfig) -> anyhow::Result<Db> {
    if !boot.db_path().exists() {
        anyhow::bail!("no database at {} (is NODESTARR_DATA_DIR right?)", boot.db_path().display());
    }
    Ok(Db::open(&boot.db_path())?)
}

/// Sets a new password and ends the user's sessions.
pub async fn reset_password(boot: &BootConfig, username: &str, password: Option<&str>) -> anyhow::Result<()> {
    let db = open_db(boot)?;
    let password = match password {
        Some(p) => p.to_string(),
        None => crate::util::random_token(12),
    };
    crate::auth::password::validate(&password).map_err(|e| anyhow::anyhow!(e))?;
    let hash = crate::auth::password::hash(&password)?;
    let user = username.to_string();
    let changed = db
        .call(move |c| {
            let n = c.execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE username = ?3",
                rusqlite::params![hash, crate::util::now(), user],
            )?;
            c.execute("DELETE FROM sessions WHERE user_id IN (SELECT id FROM users WHERE username = ?1)", [&user])?;
            Ok(n)
        })
        .await?;
    if changed == 0 {
        anyhow::bail!("no user named '{username}'");
    }
    println!("Password for '{username}' reset to: {password}");
    Ok(())
}

/// Turns off two-factor authentication for a user.
pub async fn disable_2fa(boot: &BootConfig, username: &str) -> anyhow::Result<()> {
    let db = open_db(boot)?;
    let user = username.to_string();
    let changed = db
        .call(move |c| {
            Ok(c.execute(
                "UPDATE users SET totp_enabled = 0, totp_secret = NULL, recovery_codes = NULL, updated_at = ?1 WHERE username = ?2",
                rusqlite::params![crate::util::now(), user],
            )?)
        })
        .await?;
    if changed == 0 {
        anyhow::bail!("no user named '{username}'");
    }
    println!("Two-factor authentication disabled for '{username}'");
    Ok(())
}

/// Exit status 0 when the local server responds (Docker HEALTHCHECK).
pub async fn healthcheck(boot: &BootConfig) -> anyhow::Result<()> {
    let port = boot.listen.port();
    let host = if boot.listen.ip().is_unspecified() { "127.0.0.1".to_string() } else { boot.listen.ip().to_string() };
    // A raw HTTP/1.0 request keeps this path free of TLS and proxy settings.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream =
        tokio::time::timeout(std::time::Duration::from_secs(4), tokio::net::TcpStream::connect((host.as_str(), port)))
            .await??;
    stream.write_all(format!("GET /api/v1/health HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes()).await?;
    let mut buf = vec![0u8; 256];
    let n = tokio::time::timeout(std::time::Duration::from_secs(4), stream.read(&mut buf)).await??;
    let head = String::from_utf8_lossy(&buf[..n]);
    if head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        anyhow::bail!("unhealthy: {}", head.lines().next().unwrap_or("no response"))
    }
}
