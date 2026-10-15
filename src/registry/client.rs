//! Minimal OCI distribution client for update checks.
//!
//! Update checks use `HEAD` manifest requests, which Docker Hub does not
//! count against its pull rate limit. Manifests and config blobs (needed to
//! show "what version is the new image") are only fetched when a digest
//! changed, and are cached by digest.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, WWW_AUTHENTICATE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::registry::reference::ImageRef;
use crate::settings::SettingsStore;

const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.docker.distribution.manifest.v2+json, \
     application/vnd.oci.image.manifest.v1+json";

const DIGEST_TTL: Duration = Duration::from_secs(300);
const TAGS_TTL: Duration = Duration::from_secs(3600);
const MAX_TAGS: usize = 5000;

#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistryError {
    #[error("image or tag not found in the registry")]
    NotFound,
    #[error("registry denied access (private image? add credentials in Settings > Updates)")]
    Unauthorized,
    #[error("registry rate limit reached, try again later")]
    RateLimited,
    #[error("{0}")]
    Network(String),
    #[error("unexpected registry response: {0}")]
    Protocol(String),
}

impl From<RegistryError> for crate::error::AppError {
    fn from(e: RegistryError) -> Self {
        crate::error::AppError::Upstream(e.to_string())
    }
}

type RResult<T> = std::result::Result<T, RegistryError>;

/// Platform to select from multi-arch indexes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub variant: Option<String>,
}

impl Default for Platform {
    fn default() -> Self {
        Self {
            os: "linux".into(),
            arch: match std::env::consts::ARCH {
                "x86_64" => "amd64".into(),
                "aarch64" => "arm64".into(),
                other => other.into(),
            },
            variant: None,
        }
    }
}

/// What Nodestarr learns about a remote image.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RemoteImage {
    /// Digest of the tag (index digest for multi-arch images).
    pub digest: String,
    /// Digest of the platform-specific manifest.
    pub manifest_digest: Option<String>,
    pub version: Option<String>,
    pub created: Option<i64>,
    pub source: Option<String>,
    #[serde(skip)]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagInfo {
    pub name: String,
    /// Unix seconds, when the registry reports it (Docker Hub).
    pub updated: Option<i64>,
    pub digest: Option<String>,
}

#[derive(Clone)]
pub struct RegistryClient {
    http: reqwest::Client,
    inner: Arc<Inner>,
}

/// Cached tag lists per repository, with the time they were fetched.
type TagCache = HashMap<String, (Arc<Vec<TagInfo>>, Instant)>;

struct Inner {
    settings: SettingsStore,
    tokens: Mutex<HashMap<(String, String), (String, Instant)>>,
    digests: Mutex<HashMap<String, (String, Instant)>>,
    remote: Mutex<HashMap<String, RemoteImage>>,
    tags: Mutex<TagCache>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ManifestDoc {
    #[serde(rename = "mediaType")]
    media_type: Option<String>,
    manifests: Option<Vec<IndexEntry>>,
    config: Option<Descriptor>,
}

#[derive(Debug, Deserialize)]
struct IndexEntry {
    digest: String,
    platform: Option<PlatformDoc>,
    annotations: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct PlatformDoc {
    os: Option<String>,
    architecture: Option<String>,
    variant: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Descriptor {
    digest: String,
}

#[derive(Debug, Deserialize)]
struct ConfigBlob {
    created: Option<String>,
    config: Option<ConfigInner>,
}

#[derive(Debug, Deserialize)]
struct ConfigInner {
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
}

struct Challenge {
    scheme: String,
    params: HashMap<String, String>,
}

fn parse_challenge(header: &str) -> Option<Challenge> {
    let header = header.trim();
    let (scheme, rest) = header.split_once(' ').unwrap_or((header, ""));
    let mut params = HashMap::new();
    // key="value",key2="value2" (values may contain commas inside quotes)
    let mut rest = rest.trim();
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else { break };
        let key = key.trim().trim_start_matches(',').trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (value, remaining) = if let Some(stripped) = after.strip_prefix('"') {
            match stripped.find('"') {
                Some(end) => (&stripped[..end], &stripped[end + 1..]),
                None => (stripped, ""),
            }
        } else {
            match after.find(',') {
                Some(end) => (&after[..end], &after[end..]),
                None => (after, ""),
            }
        };
        params.insert(key, value.to_string());
        rest = remaining.trim_start_matches(',').trim();
    }
    Some(Challenge {
        scheme: scheme.to_ascii_lowercase(),
        params,
    })
}

impl RegistryClient {
    pub fn new(http: reqwest::Client, settings: SettingsStore) -> Self {
        Self {
            http,
            inner: Arc::new(Inner {
                settings,
                tokens: Mutex::new(HashMap::new()),
                digests: Mutex::new(HashMap::new()),
                remote: Mutex::new(HashMap::new()),
                tags: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Configured credentials for a registry host.
    pub fn credentials(&self, registry: &str) -> Option<(String, String)> {
        let settings = self.inner.settings.get();
        let wanted = normalize_registry(registry);
        settings
            .updates
            .registries
            .iter()
            .find(|r| normalize_registry(&r.registry) == wanted && !r.username.is_empty())
            .map(|r| (r.username.clone(), r.password.clone()))
    }

    /// Credentials in the form Docker's pull API expects.
    pub fn docker_credentials(&self, r: &ImageRef) -> Option<bollard::auth::DockerCredentials> {
        self.credentials(&r.registry).map(|(username, password)| bollard::auth::DockerCredentials {
            username: Some(username),
            password: Some(password),
            serveraddress: Some(if r.is_docker_hub() {
                "https://index.docker.io/v1/".to_string()
            } else {
                r.registry.clone()
            }),
            ..Default::default()
        })
    }

    fn base_url(r: &ImageRef) -> String {
        let host = r.api_host();
        let scheme = if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{host}/v2/{}", r.repository)
    }

    async fn token(&self, r: &ImageRef, challenge: &Challenge) -> RResult<String> {
        let key = (r.registry.clone(), r.repository.clone());
        if let Some((tok, exp)) = self.inner.tokens.lock().get(&key) {
            if *exp > Instant::now() {
                return Ok(tok.clone());
            }
        }
        let realm = challenge
            .params
            .get("realm")
            .ok_or_else(|| RegistryError::Protocol("auth challenge without realm".into()))?;
        let mut req = self.http.get(realm).query(&[("scope", format!("repository:{}:pull", r.repository))]);
        if let Some(service) = challenge.params.get("service") {
            req = req.query(&[("service", service)]);
        }
        if let Some((user, pass)) = self.credentials(&r.registry) {
            req = req.basic_auth(user, Some(pass));
        }
        let resp = req.send().await.map_err(net_err)?;
        match resp.status() {
            s if s.is_success() => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(RegistryError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => return Err(RegistryError::RateLimited),
            s => return Err(RegistryError::Protocol(format!("token endpoint returned {s}"))),
        }
        let body: TokenResponse = resp.json().await.map_err(|e| RegistryError::Protocol(e.to_string()))?;
        let token = body
            .token
            .or(body.access_token)
            .ok_or_else(|| RegistryError::Protocol("token response without token".into()))?;
        let ttl = body.expires_in.unwrap_or(60).clamp(30, 3600);
        self.inner
            .tokens
            .lock()
            .insert(key, (token.clone(), Instant::now() + Duration::from_secs(ttl.saturating_sub(10))));
        Ok(token)
    }

    /// Sends a request, handling the bearer/basic auth dance once.
    async fn send(&self, r: &ImageRef, method: reqwest::Method, url: &str, accept: Option<&str>) -> RResult<reqwest::Response> {
        let build = |auth: Option<String>| {
            let mut req = self.http.request(method.clone(), url);
            if let Some(a) = accept {
                req = req.header(ACCEPT, a);
            }
            if let Some(a) = auth {
                req = req.header(AUTHORIZATION, a);
            }
            req
        };
        // Reuse a cached token if we have one.
        let cached = self
            .inner
            .tokens
            .lock()
            .get(&(r.registry.clone(), r.repository.clone()))
            .filter(|(_, exp)| *exp > Instant::now())
            .map(|(t, _)| format!("Bearer {t}"));
        let resp = build(cached).send().await.map_err(net_err)?;
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        let header = resp
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|h| h.to_str().ok())
            .map(str::to_string)
            .ok_or(RegistryError::Unauthorized)?;
        let challenge = parse_challenge(&header).ok_or(RegistryError::Unauthorized)?;
        let auth = match challenge.scheme.as_str() {
            "bearer" => {
                self.inner.tokens.lock().remove(&(r.registry.clone(), r.repository.clone()));
                format!("Bearer {}", self.token(r, &challenge).await?)
            }
            "basic" => {
                use base64::Engine;
                let (u, p) = self.credentials(&r.registry).ok_or(RegistryError::Unauthorized)?;
                format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}")))
            }
            _ => return Err(RegistryError::Unauthorized),
        };
        build(Some(auth)).send().await.map_err(net_err)
    }

    fn check_status(resp: &reqwest::Response) -> RResult<()> {
        match resp.status() {
            s if s.is_success() => Ok(()),
            StatusCode::NOT_FOUND => Err(RegistryError::NotFound),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(RegistryError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => Err(RegistryError::RateLimited),
            s => Err(RegistryError::Protocol(format!("HTTP {s}"))),
        }
    }

    /// Current digest of a tag (cached for a few minutes unless `fresh`).
    pub async fn digest(&self, r: &ImageRef, fresh: bool) -> RResult<String> {
        let key = format!("{}:{}", r.name(), r.tag_or_latest());
        if !fresh {
            if let Some((d, at)) = self.inner.digests.lock().get(&key) {
                if at.elapsed() < DIGEST_TTL {
                    return Ok(d.clone());
                }
            }
        }
        let url = format!("{}/manifests/{}", Self::base_url(r), r.tag_or_latest());
        let resp = self.send(r, reqwest::Method::HEAD, &url, Some(MANIFEST_ACCEPT)).await?;
        Self::check_status(&resp)?;
        let digest = match digest_header(resp.headers()) {
            Some(d) => d,
            None => {
                // Some registries omit the header on HEAD; hash the body instead.
                let resp = self.send(r, reqwest::Method::GET, &url, Some(MANIFEST_ACCEPT)).await?;
                Self::check_status(&resp)?;
                let from_header = digest_header(resp.headers());
                let body = resp.bytes().await.map_err(net_err)?;
                from_header.unwrap_or_else(|| format!("sha256:{}", hex::encode(Sha256::digest(&body))))
            }
        };
        self.inner.digests.lock().insert(key, (digest.clone(), Instant::now()));
        Ok(digest)
    }

    async fn get_manifest(&self, r: &ImageRef, reference: &str) -> RResult<(String, ManifestDoc)> {
        let url = format!("{}/manifests/{reference}", Self::base_url(r));
        let resp = self.send(r, reqwest::Method::GET, &url, Some(MANIFEST_ACCEPT)).await?;
        Self::check_status(&resp)?;
        let header_digest = digest_header(resp.headers());
        let body = resp.bytes().await.map_err(net_err)?;
        let digest = header_digest.unwrap_or_else(|| format!("sha256:{}", hex::encode(Sha256::digest(&body))));
        let doc: ManifestDoc = serde_json::from_slice(&body).map_err(|e| RegistryError::Protocol(format!("manifest: {e}")))?;
        Ok((digest, doc))
    }

    /// Version, creation date and labels of the image a tag points to.
    pub async fn remote_image(&self, r: &ImageRef, platform: &Platform) -> RResult<RemoteImage> {
        let tag_digest = self.digest(r, false).await?;
        let cache_key = format!("{}@{}#{}/{}", r.name(), tag_digest, platform.os, platform.arch);
        if let Some(hit) = self.inner.remote.lock().get(&cache_key) {
            return Ok(hit.clone());
        }
        let (top_digest, top) = self.get_manifest(r, &tag_digest).await?;
        let (manifest_digest, manifest) = if top.manifests.is_some() {
            let entries = top.manifests.unwrap_or_default();
            let chosen = pick_platform(&entries, platform)
                .ok_or_else(|| RegistryError::Protocol(format!("no {}/{} image in this tag", platform.os, platform.arch)))?;
            let digest = chosen.digest.clone();
            let (_, m) = self.get_manifest(r, &digest).await?;
            (Some(digest), m)
        } else {
            (None, top)
        };
        let _ = manifest.media_type;
        let mut remote = RemoteImage {
            digest: top_digest,
            manifest_digest,
            ..Default::default()
        };
        if let Some(cfg) = manifest.config {
            let url = format!("{}/blobs/{}", Self::base_url(r), cfg.digest);
            let resp = self.send(r, reqwest::Method::GET, &url, None).await?;
            Self::check_status(&resp)?;
            let blob: ConfigBlob = resp
                .json()
                .await
                .map_err(|e| RegistryError::Protocol(format!("image config: {e}")))?;
            remote.created = blob.created.as_deref().and_then(crate::util::parse_rfc3339);
            remote.labels = blob.config.and_then(|c| c.labels).unwrap_or_default();
            remote.version = version_from_labels(&remote.labels);
            remote.source = ["org.opencontainers.image.source", "org.opencontainers.image.url"]
                .iter()
                .find_map(|k| remote.labels.get(*k).cloned());
        }
        let mut cache = self.inner.remote.lock();
        if cache.len() > 500 {
            cache.clear();
        }
        cache.insert(cache_key, remote.clone());
        Ok(remote)
    }

    /// All tags of a repository, newest first when the registry tells us.
    pub async fn tags(&self, r: &ImageRef, fresh: bool) -> RResult<Arc<Vec<TagInfo>>> {
        let key = r.name();
        if !fresh {
            if let Some((tags, at)) = self.inner.tags.lock().get(&key) {
                if at.elapsed() < TAGS_TTL {
                    return Ok(tags.clone());
                }
            }
        }
        let tags = if r.is_docker_hub() {
            match self.hub_tags(r).await {
                Ok(t) => t,
                Err(_) => self.registry_tags(r).await?,
            }
        } else {
            self.registry_tags(r).await?
        };
        let tags = Arc::new(tags);
        self.inner.tags.lock().insert(key, (tags.clone(), Instant::now()));
        Ok(tags)
    }

    async fn registry_tags(&self, r: &ImageRef) -> RResult<Vec<TagInfo>> {
        let mut out = Vec::new();
        let base = Self::base_url(r);
        let mut url = format!("{base}/tags/list?n=1000");
        for _ in 0..20 {
            let resp = self.send(r, reqwest::Method::GET, &url, None).await?;
            Self::check_status(&resp)?;
            let next = resp
                .headers()
                .get("link")
                .and_then(|h| h.to_str().ok())
                .and_then(parse_next_link);
            #[derive(Deserialize)]
            struct TagList {
                tags: Option<Vec<String>>,
            }
            let list: TagList = resp.json().await.map_err(|e| RegistryError::Protocol(e.to_string()))?;
            out.extend(list.tags.unwrap_or_default().into_iter().map(|name| TagInfo {
                name,
                updated: None,
                digest: None,
            }));
            match next {
                Some(n) if out.len() < MAX_TAGS => {
                    url = if n.starts_with("http") {
                        n
                    } else {
                        let host = r.api_host();
                        format!("https://{host}{n}")
                    };
                }
                _ => break,
            }
        }
        // Without dates, put version tags newest first.
        let mut names: Vec<String> = out.iter().map(|t| t.name.clone()).collect();
        crate::registry::version::sort_tags_desc(&mut names);
        Ok(names
            .into_iter()
            .map(|name| TagInfo {
                name,
                updated: None,
                digest: None,
            })
            .collect())
    }

    /// Docker Hub's own API includes last-updated dates and digests.
    async fn hub_tags(&self, r: &ImageRef) -> RResult<Vec<TagInfo>> {
        #[derive(Deserialize)]
        struct Page {
            next: Option<String>,
            results: Vec<HubTag>,
        }
        #[derive(Deserialize)]
        struct HubTag {
            name: String,
            last_updated: Option<String>,
            digest: Option<String>,
        }
        let (ns, repo) = r.repository.split_once('/').unwrap_or(("library", &r.repository));
        let mut url = format!(
            "https://hub.docker.com/v2/namespaces/{ns}/repositories/{repo}/tags?page_size=100&ordering=last_updated"
        );
        let mut out = Vec::new();
        for _ in 0..10 {
            let mut req = self.http.get(&url);
            if let Some((u, p)) = self.credentials("docker.io") {
                req = req.basic_auth(u, Some(p));
            }
            let resp = req.send().await.map_err(net_err)?;
            Self::check_status(&resp)?;
            let page: Page = resp.json().await.map_err(|e| RegistryError::Protocol(e.to_string()))?;
            out.extend(page.results.into_iter().map(|t| TagInfo {
                name: t.name,
                updated: t.last_updated.as_deref().and_then(crate::util::parse_rfc3339),
                digest: t.digest,
            }));
            match page.next {
                Some(n) if out.len() < 1000 => url = n,
                _ => break,
            }
        }
        Ok(out)
    }

    /// Drops cached digests so the next check hits the registry.
    pub fn forget_digests(&self) {
        self.inner.digests.lock().clear();
    }
}

fn normalize_registry(r: &str) -> String {
    let r = r
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    match r.as_str() {
        "" | "docker.io" | "index.docker.io" | "registry-1.docker.io" | "hub.docker.com" | "index.docker.io/v1" => {
            "docker.io".into()
        }
        _ => r,
    }
}

fn digest_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("docker-content-digest")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("sha256:"))
        .map(str::to_string)
}

fn net_err(e: reqwest::Error) -> RegistryError {
    RegistryError::Network(crate::util::describe_reqwest_error(&e))
}

fn parse_next_link(link: &str) -> Option<String> {
    // </v2/name/tags/list?last=x&n=1000>; rel="next"
    for part in link.split(',') {
        if part.contains("rel=\"next\"") {
            let start = part.find('<')? + 1;
            let end = part.find('>')?;
            return Some(part[start..end].to_string());
        }
    }
    None
}

fn pick_platform<'a>(entries: &'a [IndexEntry], p: &Platform) -> Option<&'a IndexEntry> {
    let is_attestation = |e: &IndexEntry| {
        e.annotations
            .as_ref()
            .is_some_and(|a| a.contains_key("vnd.docker.reference.type"))
    };
    let matches = |e: &IndexEntry, check_variant: bool| {
        e.platform.as_ref().is_some_and(|pl| {
            pl.os.as_deref() == Some(p.os.as_str())
                && pl.architecture.as_deref() == Some(p.arch.as_str())
                && (!check_variant || p.variant.is_none() || pl.variant == p.variant)
        })
    };
    entries
        .iter()
        .filter(|e| !is_attestation(e))
        .find(|e| matches(e, true))
        .or_else(|| entries.iter().filter(|e| !is_attestation(e)).find(|e| matches(e, false)))
}

/// Version from OCI/LinuxServer labels of a remote image config.
pub fn version_from_labels(labels: &HashMap<String, String>) -> Option<String> {
    if let Some(v) = labels.get("org.opencontainers.image.version").filter(|v| !v.is_empty() && v.len() < 64) {
        return Some(v.clone());
    }
    if let Some(bv) = labels.get("build_version") {
        if let Some(rest) = bv.split("version:-").nth(1) {
            return rest.split_whitespace().next().map(str::to_string);
        }
    }
    labels.get("version").filter(|v| !v.is_empty() && v.len() < 64).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bearer_challenge() {
        let c = parse_challenge(
            r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/nginx:pull""#,
        )
        .unwrap();
        assert_eq!(c.scheme, "bearer");
        assert_eq!(c.params["realm"], "https://auth.docker.io/token");
        assert_eq!(c.params["service"], "registry.docker.io");
        assert_eq!(c.params["scope"], "repository:library/nginx:pull");
        let b = parse_challenge(r#"Basic realm="Registry""#).unwrap();
        assert_eq!(b.scheme, "basic");
    }

    #[test]
    fn link_header() {
        assert_eq!(
            parse_next_link(r#"</v2/a/b/tags/list?last=1.2&n=1000>; rel="next""#).as_deref(),
            Some("/v2/a/b/tags/list?last=1.2&n=1000")
        );
        assert_eq!(parse_next_link(r#"<x>; rel="prev""#), None);
    }

    #[test]
    fn normalizes_registry_names() {
        assert_eq!(normalize_registry("https://index.docker.io/v1/"), "docker.io");
        assert_eq!(normalize_registry("GHCR.io"), "ghcr.io");
    }

    #[test]
    fn version_labels() {
        let mut l = HashMap::new();
        l.insert("build_version".into(), "Linuxserver.io version:- 1.41.0.8992-8463ad060-ls250 Build-date:- x".into());
        assert_eq!(version_from_labels(&l).as_deref(), Some("1.41.0.8992-8463ad060-ls250"));
    }

    #[test]
    fn platform_selection_skips_attestations() {
        let entries = vec![
            IndexEntry {
                digest: "sha256:att".into(),
                platform: Some(PlatformDoc { os: Some("unknown".into()), architecture: Some("unknown".into()), variant: None }),
                annotations: Some(HashMap::from([("vnd.docker.reference.type".into(), "attestation-manifest".into())])),
            },
            IndexEntry {
                digest: "sha256:arm".into(),
                platform: Some(PlatformDoc { os: Some("linux".into()), architecture: Some("arm64".into()), variant: Some("v8".into()) }),
                annotations: None,
            },
            IndexEntry {
                digest: "sha256:amd".into(),
                platform: Some(PlatformDoc { os: Some("linux".into()), architecture: Some("amd64".into()), variant: None }),
                annotations: None,
            },
        ];
        let p = Platform { os: "linux".into(), arch: "amd64".into(), variant: None };
        assert_eq!(pick_platform(&entries, &p).unwrap().digest, "sha256:amd");
        let arm = Platform { os: "linux".into(), arch: "arm64".into(), variant: Some("v8".into()) };
        assert_eq!(pick_platform(&entries, &arm).unwrap().digest, "sha256:arm");
    }
}
