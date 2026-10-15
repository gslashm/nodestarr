//! Image metadata, cached by image id.

use std::collections::HashMap;

use bollard::models::ImageInspect;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImageMeta {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub labels: HashMap<String, String>,
    pub env: HashMap<String, String>,
    pub created: Option<i64>,
    pub os: String,
    pub arch: String,
    pub variant: Option<String>,
    pub size: i64,
    /// Default command/entrypoint/user etc. from the image, used when
    /// recreating standalone containers to tell user settings from image defaults.
    #[serde(skip)]
    pub config: Option<bollard::models::ImageConfig>,
}

impl ImageMeta {
    pub fn from_inspect(i: ImageInspect) -> Self {
        let config = i.config.clone();
        let labels = config.as_ref().and_then(|c| c.labels.clone()).unwrap_or_default();
        let env = config
            .as_ref()
            .and_then(|c| c.env.as_ref())
            .map(|vars| {
                vars.iter().filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
            })
            .unwrap_or_default();
        Self {
            id: i.id.unwrap_or_default(),
            repo_tags: i.repo_tags.unwrap_or_default(),
            repo_digests: i.repo_digests.unwrap_or_default(),
            labels,
            env,
            created: i.created.as_ref().and_then(|d| crate::util::parse_rfc3339(&d.to_string())),
            os: i.os.unwrap_or_else(|| "linux".into()),
            arch: i.architecture.unwrap_or_else(|| "amd64".into()),
            variant: i.variant.filter(|v| !v.is_empty()),
            size: i.size.unwrap_or(0),
            config,
        }
    }

    /// Source repository URL from OCI labels (used for release notes).
    pub fn source_url(&self) -> Option<&str> {
        ["org.opencontainers.image.source", "org.opencontainers.image.url", "org.label-schema.vcs-url"]
            .iter()
            .find_map(|k| self.labels.get(*k))
            .map(String::as_str)
            .filter(|u| u.starts_with("http"))
    }
}

/// Summary row for the Images page.
#[derive(Debug, Clone, Serialize)]
pub struct ImageRow {
    pub id: String,
    pub tags: Vec<String>,
    pub digests: Vec<String>,
    pub size: i64,
    pub created: i64,
    pub containers: Vec<String>,
    pub dangling: bool,
    pub rollback: bool,
}
