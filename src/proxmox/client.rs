//! Proxmox VE API client (API token auth, optional certificate pinning).

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::settings::{Proxmox as ProxmoxSettings, TlsMode};

/// Formats a SHA-256 digest as "AB:CD:...".
pub fn format_fingerprint(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn parse_fingerprint(s: &str) -> Option<Vec<u8>> {
    let hex_str: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex_str.len() != 64 {
        return None;
    }
    hex::decode(hex_str).ok()
}

/// Accepts exactly one certificate (by SHA-256), or records whatever the
/// server presents when `record` is set (used by "Test connection").
#[derive(Debug)]
struct PinVerifier {
    expected: Option<Vec<u8>>,
    record: Option<Arc<Mutex<Option<String>>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let digest = Sha256::digest(end_entity.as_ref());
        if let Some(rec) = &self.record {
            *rec.lock() = Some(format_fingerprint(&digest));
        }
        match &self.expected {
            Some(expected) if expected.as_slice() == digest.as_slice() => Ok(ServerCertVerified::assertion()),
            Some(_) => Err(rustls::Error::General(
                "certificate fingerprint does not match the pinned one".into(),
            )),
            None if self.record.is_some() => Ok(ServerCertVerified::assertion()),
            None => Err(rustls::Error::General("no certificate fingerprint configured".into())),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn pinned_config(expected: Option<Vec<u8>>, record: Option<Arc<Mutex<Option<String>>>>) -> Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = PinVerifier {
        expected,
        record,
        provider: provider.clone(),
    };
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::Other(anyhow::anyhow!("TLS setup: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}

#[derive(Clone)]
pub struct PveClient {
    http: reqwest::Client,
    base: String,
    auth: String,
}

impl PveClient {
    pub fn new(s: &ProxmoxSettings) -> Result<Self> {
        let builder = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(1);
        let builder = match s.tls_mode {
            TlsMode::Verify => builder,
            TlsMode::Insecure => builder.tls_danger_accept_invalid_certs(true),
            TlsMode::Fingerprint => {
                let fp = parse_fingerprint(&s.fingerprint).ok_or_else(|| {
                    AppError::bad_request("set the Proxmox certificate fingerprint (use Test connection to fetch it)")
                })?;
                builder.tls_backend_preconfigured(pinned_config(Some(fp), None)?)
            }
        };
        Ok(Self {
            http: builder.build().map_err(|e| AppError::Other(e.into()))?,
            base: format!("{}/api2/json", s.url.trim_end_matches('/')),
            auth: format!("PVEAPIToken={}={}", s.token_id.trim(), s.token_secret.trim()),
        })
    }

    async fn handle<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::upstream("Proxmox rejected the API token"));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::upstream(
                "the API token lacks permission for this (see docs/proxmox.md for the required roles)",
            ));
        }
        let body: Value = resp.json().await.map_err(|e| AppError::upstream(format!("Proxmox: {e}")))?;
        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| body.get("errors").map(|e| e.to_string()))
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(AppError::upstream(format!("Proxmox: {}", msg.trim())));
        }
        serde_json::from_value(body.get("data").cloned().unwrap_or(Value::Null))
            .map_err(|e| AppError::upstream(format!("unexpected Proxmox response: {e}")))
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send()
            .await?;
        Self::handle(resp).await
    }

    pub async fn post<T: DeserializeOwned>(&self, path: &str, form: &[(&str, String)]) -> Result<T> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .form(form)
            .send()
            .await?;
        Self::handle(resp).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .delete(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send()
            .await?;
        Self::handle(resp).await
    }
}

/// Connects without trusting anything and reports the server certificate's
/// fingerprint, so the user can confirm and pin it.
pub async fn fetch_fingerprint(url: &str) -> Result<String> {
    let seen = Arc::new(Mutex::new(None));
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .tls_backend_preconfigured(pinned_config(None, Some(seen.clone()))?)
        .build()
        .map_err(|e| AppError::Other(e.into()))?;
    let _ = http.get(format!("{}/api2/json/version", url.trim_end_matches('/'))).send().await;
    let fp = seen.lock().clone();
    fp.ok_or_else(|| AppError::upstream("could not reach Proxmox to read its certificate"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_formats() {
        let fp = format_fingerprint(&[0xab, 0x01, 0xff]);
        assert_eq!(fp, "AB:01:FF");
        let full = "AB:".repeat(31) + "AB";
        assert_eq!(parse_fingerprint(&full).unwrap().len(), 32);
        assert!(parse_fingerprint("nope").is_none());
    }
}
