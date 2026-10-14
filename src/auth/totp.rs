//! Time-based one-time passwords (RFC 6238): SHA-1, 6 digits, 30 seconds,
//! compatible with every authenticator app.

use hmac::{Hmac, KeyInit, Mac};

type HmacSha1 = Hmac<sha1::Sha1>;

const STEP: i64 = 30;
const DIGITS: u32 = 6;

/// A new random secret, base32 encoded (no padding).
pub fn new_secret() -> String {
    data_encoding::BASE32_NOPAD.encode(&crate::util::random_bytes::<20>())
}

pub fn otpauth_uri(secret: &str, account: &str, issuer: &str) -> String {
    let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
    format!(
        "otpauth://totp/{}:{}?secret={secret}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP}",
        enc(issuer),
        enc(account),
        enc(issuer)
    )
}

fn code_at(key: &[u8], counter: u64) -> u32 {
    let mut mac = <HmacSha1 as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let bin = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    bin % 10u32.pow(DIGITS)
}

/// Checks a code with ±1 step tolerance for clock drift. Returns the time
/// step that matched so callers can reject replays of the same code.
pub fn verify(secret_b32: &str, code: &str, now: i64) -> Option<i64> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let wanted: u32 = code.parse().ok()?;
    let key = data_encoding::BASE32_NOPAD
        .decode(secret_b32.trim_end_matches('=').to_ascii_uppercase().as_bytes())
        .ok()?;
    let step = now / STEP;
    (-1..=1)
        .map(|d| step + d)
        .find(|s| *s >= 0 && code_at(&key, *s as u64) == wanted)
}

/// The current code for a secret (used by tests and the setup preview).
pub fn code_for(secret_b32: &str, now: i64) -> Option<String> {
    let key = data_encoding::BASE32_NOPAD.decode(secret_b32.trim_end_matches('=').as_bytes()).ok()?;
    Some(format!("{:06}", code_at(&key, (now / STEP) as u64)))
}

/// Ten single-use recovery codes like "k3m9-x2pq".
pub fn recovery_codes() -> Vec<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    (0..10)
        .map(|_| {
            let bytes = crate::util::random_bytes::<8>();
            let chars: String = bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
            format!("{}-{}", &chars[..4], &chars[4..])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vectors() {
        // RFC 6238 test secret "12345678901234567890" (SHA-1), 8-digit vectors
        // truncated to 6 digits.
        let key = b"12345678901234567890";
        assert_eq!(code_at(key, 59 / 30), 287082);
        assert_eq!(code_at(key, 1111111109 / 30), 81804);
        assert_eq!(code_at(key, 1234567890 / 30), 5924);
    }

    #[test]
    fn verify_with_drift() {
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(verify(&secret, "287082", 59), Some(1));
        assert_eq!(verify(&secret, "287 082", 59 + 30), Some(1));
        assert_eq!(verify(&secret, "287082", 59 + 90), None);
        assert_eq!(verify(&secret, "abc", 59), None);
        let codes = recovery_codes();
        assert_eq!(codes.len(), 10);
        assert_eq!(codes[0].len(), 9);
    }

    #[test]
    fn uri() {
        let u = otpauth_uri("ABC", "jake", "Berth");
        assert!(u.starts_with("otpauth://totp/Berth:jake?secret=ABC&issuer=Berth"));
    }
}
