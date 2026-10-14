//! Password hashing with Argon2id.

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};

/// OWASP-recommended Argon2id parameters (19 MiB, 2 iterations).
fn argon() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(19 * 1024, 2, 1, None).expect("valid params"))
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

/// A precomputed hash used to make "unknown user" take as long as "wrong password".
pub fn dummy_hash() -> &'static str {
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
}
