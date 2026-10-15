//! Password hashing with Argon2id.
//!
//! Every hash or check uses 19 MiB of memory and tens of milliseconds of CPU,
//! so the server goes through [`hash_async`] and [`verify_async`]: they run on
//! the blocking pool, at most two at a time, so a burst of sign-in attempts
//! can neither stall the async workers nor run up memory.

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};
use tokio::sync::Semaphore;

/// OWASP-recommended Argon2id parameters (19 MiB, 2 iterations).
fn argon() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(19 * 1024, 2, 1, None).expect("valid params"))
}

/// Limits concurrent hashes and checks.
static HASHING: Semaphore = Semaphore::const_new(2);

async fn limited<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> anyhow::Result<T> {
    let _permit = HASHING.acquire().await?;
    Ok(tokio::task::spawn_blocking(work).await?)
}

/// [`hash`] on the blocking pool.
pub async fn hash_async(password: &str) -> anyhow::Result<String> {
    let password = password.to_string();
    limited(move || hash(&password)).await?
}

/// [`verify`] on the blocking pool.
pub async fn verify_async(password: &str, stored: &str) -> bool {
    let (password, stored) = (password.to_string(), stored.to_string());
    limited(move || verify(&password, &stored)).await.unwrap_or(false)
}

/// Does the work of [`verify_async`] against a made-up hash, so rejecting an
/// unknown username takes as long as rejecting a wrong password.
pub async fn verify_dummy(password: &str) {
    let password = password.to_string();
    let _ = limited(move || verify(&password, dummy_hash())).await;
}

pub fn validate(password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("passwords need at least 8 characters".into());
    }
    if password.len() > 256 {
        return Err("password is too long".into());
    }
    Ok(())
}

pub fn hash(password: &str) -> anyhow::Result<String> {
    let salt = crate::util::random_bytes::<16>();
    let hash: argon2::PasswordHash = argon()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify(password: &str, stored: &str) -> bool {
    PasswordVerifier::<str>::verify_password(&argon(), password.as_bytes(), stored).is_ok()
}

/// A hash of a password nobody has, computed once.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| hash("not-a-real-password").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let h = hash("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse", &h));
        assert!(!verify("wrong horse", &h));
        assert!(!verify("x", "garbage"));
        assert!(validate("short").is_err());
        assert!(validate("long enough").is_ok());
    }

    #[tokio::test]
    async fn async_helpers_agree_with_the_sync_ones() {
        let h = hash_async("correct horse").await.unwrap();
        assert!(verify("correct horse", &h));
        assert!(verify_async("correct horse", &h).await);
        assert!(!verify_async("wrong horse", &h).await);
        verify_dummy("correct horse").await;
    }
}
