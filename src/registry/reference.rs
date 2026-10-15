//! Docker image reference parsing ("lscr.io/linuxserver/sonarr:develop").

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRef {
    /// Registry domain as written ("docker.io" when omitted).
    pub registry: String,
    /// Repository path; Docker Hub official images get the "library/" prefix.
    pub repository: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
}

impl ImageRef {
    /// Parses a reference. Returns `None` for empty or malformed input.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() || input.contains(char::is_whitespace) {
            return None;
        }
        let (name_tag, digest) = match input.split_once('@') {
            Some((n, d)) if d.contains(':') => (n, Some(d.to_string())),
            Some(_) => return None,
            None => (input, None),
        };
        // The tag is after the last ':' that comes after the last '/'.
        let last_slash = name_tag.rfind('/').map(|i| i + 1).unwrap_or(0);
        let (name, tag) = match name_tag[last_slash..].rfind(':') {
            Some(i) => {
                let idx = last_slash + i;
                (&name_tag[..idx], Some(name_tag[idx + 1..].to_string()))
            }
            None => (name_tag, None),
        };
        if name.is_empty() || tag.as_deref() == Some("") {
            return None;
        }
        let mut parts: Vec<&str> = name.split('/').collect();
        let registry =
            if parts.len() > 1 && (parts[0].contains('.') || parts[0].contains(':') || parts[0] == "localhost") {
                parts.remove(0).to_string()
            } else {
                "docker.io".to_string()
            };
        let registry = match registry.as_str() {
            "index.docker.io" | "registry-1.docker.io" | "registry.hub.docker.com" => "docker.io".to_string(),
            _ => registry,
        };
        let mut repository = parts.join("/");
        if repository.is_empty() {
            return None;
        }
        if registry == "docker.io" && !repository.contains('/') {
            repository = format!("library/{repository}");
        }
        Some(Self { registry, repository: repository.to_ascii_lowercase(), tag, digest })
    }

    /// The tag, defaulting to "latest" like Docker does.
    pub fn tag_or_latest(&self) -> &str {
        self.tag.as_deref().unwrap_or("latest")
    }

    /// Last path component of the repository ("sonarr" for linuxserver/sonarr).
    pub fn repo_basename(&self) -> &str {
        self.repository.rsplit('/').next().unwrap_or(&self.repository)
    }

    pub fn is_docker_hub(&self) -> bool {
        self.registry == "docker.io"
    }

    /// Host to talk to for the registry API.
    pub fn api_host(&self) -> &str {
        if self.is_docker_hub() { "registry-1.docker.io" } else { &self.registry }
    }

    /// `registry/repository` without tag, normalized.
    pub fn name(&self) -> String {
        format!("{}/{}", self.registry, self.repository)
    }

    pub fn with_tag(&self, tag: &str) -> Self {
        Self {
            registry: self.registry.clone(),
            repository: self.repository.clone(),
            tag: Some(tag.to_string()),
            digest: None,
        }
    }

    /// Short form as people write it: "nginx:1.27", "linuxserver/sonarr:latest",
    /// "ghcr.io/hotio/radarr:release".
    pub fn familiar(&self) -> String {
        let mut s = if self.is_docker_hub() {
            self.repository.strip_prefix("library/").unwrap_or(&self.repository).to_string()
        } else {
            format!("{}/{}", self.registry, self.repository)
        };
        s.push(':');
        s.push_str(self.tag_or_latest());
        if let Some(d) = &self.digest {
            s.push('@');
            s.push_str(d);
        }
        s
    }

    /// If `repo_digest` ("name@sha256:...") refers to the same repository,
    /// returns its digest. Docker records one RepoDigest per repository an
    /// image was pulled from.
    pub fn digest_from_repo_digest(&self, repo_digest: &str) -> Option<String> {
        let (name, digest) = repo_digest.split_once('@')?;
        let other = ImageRef::parse(name)?;
        if other.registry == self.registry && other.repository == self.repository {
            Some(digest.to_string())
        } else {
            None
        }
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.familiar())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_images() {
        let r = ImageRef::parse("nginx").unwrap();
        assert_eq!(r.registry, "docker.io");
        assert_eq!(r.repository, "library/nginx");
        assert_eq!(r.tag, None);
        assert_eq!(r.tag_or_latest(), "latest");
        assert_eq!(r.familiar(), "nginx:latest");
        assert_eq!(r.api_host(), "registry-1.docker.io");
    }

    #[test]
    fn parses_namespaced_and_registries() {
        let r = ImageRef::parse("lscr.io/linuxserver/sonarr:develop").unwrap();
        assert_eq!(r.registry, "lscr.io");
        assert_eq!(r.repository, "linuxserver/sonarr");
        assert_eq!(r.tag.as_deref(), Some("develop"));
        assert_eq!(r.repo_basename(), "sonarr");

        let r = ImageRef::parse("localhost:5000/team/app:1.2").unwrap();
        assert_eq!(r.registry, "localhost:5000");
        assert_eq!(r.repository, "team/app");
        assert_eq!(r.tag.as_deref(), Some("1.2"));

        let r = ImageRef::parse("linuxserver/plex").unwrap();
        assert_eq!(r.repository, "linuxserver/plex");
        assert_eq!(r.familiar(), "linuxserver/plex:latest");

        let r = ImageRef::parse("ghcr.io/immich-app/immich-server:release@sha256:abc").unwrap();
        assert_eq!(r.digest.as_deref(), Some("sha256:abc"));
        assert_eq!(r.tag.as_deref(), Some("release"));

        assert!(ImageRef::parse("").is_none());
        assert!(ImageRef::parse("bad image").is_none());
        assert!(ImageRef::parse("nginx:").is_none());
    }

    #[test]
    fn repo_digest_matching() {
        let r = ImageRef::parse("lscr.io/linuxserver/sonarr:latest").unwrap();
        assert_eq!(r.digest_from_repo_digest("lscr.io/linuxserver/sonarr@sha256:111").as_deref(), Some("sha256:111"));
        assert_eq!(r.digest_from_repo_digest("ghcr.io/linuxserver/sonarr@sha256:222"), None);
        let hub = ImageRef::parse("nginx:1.27").unwrap();
        assert_eq!(hub.digest_from_repo_digest("nginx@sha256:333").as_deref(), Some("sha256:333"));
        assert_eq!(hub.digest_from_repo_digest("docker.io/library/nginx@sha256:333").as_deref(), Some("sha256:333"));
    }
}
