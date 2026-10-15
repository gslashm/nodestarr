//! Docker Compose stacks: discovery, the `docker compose` CLI runner,
//! resolved configuration, stack file editing and image tag changes.
//!
//! Nodestarr drives the real `docker compose` binary rather than reimplementing
//! Compose, so behavior always matches what you get in a shell.

pub mod envfile;
pub mod yaml_edit;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::docker::DockerService;
use crate::docker::model::LABEL_ONEOFF;
use crate::error::{AppError, Result};
use crate::settings::SettingsStore;
use crate::tasks::TaskCtx;

/// File names compose looks for, in order of preference.
pub const COMPOSE_FILE_NAMES: &[&str] = &["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"];
const OVERRIDE_FILE_NAMES: &[&str] =
    &["compose.override.yaml", "compose.override.yml", "docker-compose.override.yaml", "docker-compose.override.yml"];

/// Everything needed to run compose commands for a stack.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StackRef {
    pub name: String,
    pub dir: String,
    pub files: Vec<String>,
    /// Explicit env file when it is not `<dir>/.env`.
    pub env_file: Option<String>,
}

impl StackRef {
    /// The env file compose reads for interpolation.
    pub fn env_path(&self) -> PathBuf {
        self.env_file.as_ref().map(PathBuf::from).unwrap_or_else(|| Path::new(&self.dir).join(".env"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StackContainer {
    pub id: String,
    pub name: String,
    pub identity: String,
    pub service: Option<String>,
    pub state: String,
    pub health: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackSummary {
    pub name: String,
    pub dir: Option<String>,
    pub files: Vec<String>,
    /// "running", "partial", "stopped" or "inactive" (never deployed / down).
    pub status: &'static str,
    pub running: usize,
    pub total: usize,
    pub containers: Vec<StackContainer>,
    /// Compose files exist and are readable by Nodestarr.
    pub editable: bool,
    /// Found in one of the configured stacks folders.
    pub in_stacks_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedService {
    pub image: Option<String>,
    pub container_name: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    pub network_mode: Option<String>,
    #[serde(default)]
    pub depends_on: serde_json::Value,
    pub build: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedProject {
    pub name: Option<String>,
    #[serde(default)]
    pub services: BTreeMap<String, ResolvedService>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackFile {
    /// File name relative to the stack folder (or absolute when outside it).
    pub name: String,
    pub path: String,
    /// "compose" or "env".
    pub kind: &'static str,
    pub content: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEdit {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub before: String,
    pub after: String,
    /// The file didn't exist before (an `.env` created for a tag switch), so
    /// undoing the change removes it.
    #[serde(default)]
    pub created: bool,
}

impl FileChange {
    /// Puts the file back the way it was before this change.
    pub async fn revert(&self, db: &crate::db::Db, actor: &crate::activity::Actor, note: &str) -> Result<()> {
        let path = Path::new(&self.path);
        if self.created {
            // Keep the removed content in the file history, then delete.
            crate::files::record_version(db, &self.path, self.after.as_bytes(), actor, Some(note.to_string())).await?;
            match tokio::fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        } else {
            crate::files::write_with_history(db, path, &self.before, actor, Some(note.to_string())).await.map(|_| ())
        }
    }
}

#[derive(Clone)]
pub struct ComposeService {
    inner: Arc<Inner>,
}

struct Inner {
    settings: SettingsStore,
    docker: DockerService,
    data_dir: PathBuf,
    command: RwLock<Option<Vec<String>>>,
    version: RwLock<Option<String>>,
    config_cache: Mutex<HashMap<String, (String, Arc<ResolvedProject>)>>,
}

/// Compose project names: lowercase letters, digits, '-' and '_'.
pub fn normalize_project_name(raw: &str) -> String {
    let s: String =
        raw.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    s.trim_start_matches(['-', '_']).to_string()
}

pub fn valid_stack_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Reads the top-level `name:` of a compose file, if any.
fn declared_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    yaml_edit::top_level_scalar(&text, "name").map(|n| normalize_project_name(&n))
}

fn find_compose_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Some(main) = COMPOSE_FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file()) {
        files.push(main.to_string_lossy().into_owned());
        if let Some(ov) = OVERRIDE_FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file()) {
            files.push(ov.to_string_lossy().into_owned());
        }
    }
    files
}

impl ComposeService {
    pub fn new(settings: SettingsStore, docker: DockerService, data_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                settings,
                docker,
                data_dir,
                command: RwLock::new(None),
                version: RwLock::new(None),
                config_cache: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn version(&self) -> Option<String> {
        self.inner.version.read().clone()
    }

    pub fn available(&self) -> bool {
        self.inner.command.read().is_some()
    }

    /// Finds a working compose command. Returns its version.
    pub async fn detect(&self) -> Option<String> {
        let configured = self.inner.settings.get().docker.compose_command.trim().to_string();
        let candidates: Vec<Vec<String>> = if configured.is_empty() {
            vec![
                vec!["docker".into(), "compose".into()],
                vec!["docker-compose".into()],
                vec!["/usr/libexec/docker/cli-plugins/docker-compose".into()],
            ]
        } else {
            vec![shell_words::split(&configured).unwrap_or_else(|_| vec![configured.clone()])]
        };
        for cmd in candidates {
            let mut c = tokio::process::Command::new(&cmd[0]);
            c.args(&cmd[1..]).args(["version", "--short"]).stdin(Stdio::null());
            if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(15), c.output()).await
                && out.status.success()
            {
                let v = String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('v').to_string();
                *self.inner.command.write() = Some(cmd);
                *self.inner.version.write() = Some(v.clone());
                self.inner.docker.set_compose_version(Some(v.clone()));
                return Some(v);
            }
        }
        *self.inner.command.write() = None;
        None
    }

    fn command(&self) -> Result<Vec<String>> {
        self.inner.command.read().clone().ok_or_else(|| {
            AppError::conflict("docker compose is not available to Nodestarr (install the compose plugin or set the compose command in Settings > Docker)")
        })
    }

    /// All stacks: those Docker knows about plus folders in the stacks dirs.
    pub fn list(&self) -> Vec<StackSummary> {
        let mut stacks: BTreeMap<String, StackSummary> = BTreeMap::new();
        for e in self.inner.docker.entries() {
            let s = &e.summary;
            let Some(project) = s.stack.clone() else { continue };
            let labels = e.inspect.config.as_ref().and_then(|c| c.labels.as_ref());
            if labels.and_then(|l| l.get(LABEL_ONEOFF)).is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                continue;
            }
            let entry = stacks.entry(project.clone()).or_insert_with(|| StackSummary {
                name: project.clone(),
                dir: s.compose_dir.clone(),
                files: s.compose_files.clone(),
                status: "stopped",
                running: 0,
                total: 0,
                containers: Vec::new(),
                editable: false,
                in_stacks_dir: false,
            });
            if entry.dir.is_none() {
                entry.dir = s.compose_dir.clone();
            }
            if entry.files.is_empty() {
                entry.files = s.compose_files.clone();
            }
            entry.total += 1;
            if s.is_running() {
                entry.running += 1;
            }
            entry.containers.push(StackContainer {
                id: s.id.clone(),
                name: s.name.clone(),
                identity: s.identity.clone(),
                service: s.service.clone(),
                state: s.state.clone(),
                health: s.health.clone(),
            });
        }

        let dirs = self.inner.settings.get().docker.stacks_dirs.clone();
        for base in dirs {
            let Ok(read) = std::fs::read_dir(&base) else { continue };
            for item in read.flatten() {
                let path = item.path();
                let fname = item.file_name().to_string_lossy().into_owned();
                if fname.starts_with('.') || !path.is_dir() {
                    continue;
                }
                let files = find_compose_files(&path);
                if files.is_empty() {
                    continue;
                }
                let dir_str = path.to_string_lossy().into_owned();
                // Match a running project by folder first, then by name.
                let existing = stacks
                    .values_mut()
                    .find(|s| s.dir.as_deref().map(|d| d.trim_end_matches('/')) == Some(dir_str.trim_end_matches('/')));
                if let Some(s) = existing {
                    s.in_stacks_dir = true;
                    continue;
                }
                let name = declared_name(Path::new(&files[0])).unwrap_or_else(|| normalize_project_name(&fname));
                let entry = stacks.entry(name.clone()).or_insert_with(|| StackSummary {
                    name: name.clone(),
                    dir: Some(dir_str.clone()),
                    files: files.clone(),
                    status: "inactive",
                    running: 0,
                    total: 0,
                    containers: Vec::new(),
                    editable: false,
                    in_stacks_dir: true,
                });
                entry.in_stacks_dir = true;
            }
        }

        for s in stacks.values_mut() {
            s.containers.sort_by(|a, b| a.name.cmp(&b.name));
            s.editable = !s.files.is_empty() && s.files.iter().all(|f| Path::new(f).is_file());
            s.status = if s.total == 0 {
                "inactive"
            } else if s.running == s.total {
                "running"
            } else if s.running == 0 {
                "stopped"
            } else {
                "partial"
            };
        }
        stacks.into_values().collect()
    }

    pub fn summary(&self, name: &str) -> Result<StackSummary> {
        self.list().into_iter().find(|s| s.name == name).ok_or_else(|| AppError::not_found(format!("stack '{name}'")))
    }

    /// Resolves a stack name into what compose needs.
    pub fn stack_ref(&self, name: &str) -> Result<StackRef> {
        let summary = self.summary(name)?;
        let dir = summary
            .dir
            .clone()
            .or_else(|| {
                summary.files.first().and_then(|f| Path::new(f).parent().map(|p| p.to_string_lossy().into_owned()))
            })
            .ok_or_else(|| AppError::conflict(format!("Nodestarr doesn't know where stack '{name}' lives")))?;
        let mut files = summary.files.clone();
        if files.is_empty() {
            files = find_compose_files(Path::new(&dir));
        }
        if files.is_empty() {
            return Err(AppError::conflict(format!("no compose file found for stack '{name}'")));
        }
        for f in &files {
            if !Path::new(f).is_file() {
                return Err(AppError::conflict(format!(
                    "compose file {f} is not visible to Nodestarr; mount your stacks folder into the Nodestarr container at the same path"
                )));
            }
        }
        // A non-default env file recorded by compose.
        let env_file = self
            .inner
            .docker
            .in_stack(name)
            .first()
            .and_then(|e| {
                e.inspect.config.as_ref()?.labels.as_ref()?.get(crate::docker::model::LABEL_ENV_FILE).cloned()
            })
            .and_then(|f| f.split(',').next().map(str::to_string))
            .filter(|f| !f.is_empty() && Path::new(f) != Path::new(&dir).join(".env") && Path::new(f).is_file());
        Ok(StackRef { name: summary.name, dir, files, env_file })
    }

    fn base_args(&self, stack: &StackRef) -> Vec<String> {
        let mut args = vec![
            "--ansi".to_string(),
            "never".to_string(),
            "--progress".to_string(),
            "plain".to_string(),
            "--project-name".to_string(),
            stack.name.clone(),
            "--project-directory".to_string(),
            stack.dir.clone(),
        ];
        for f in &stack.files {
            args.push("-f".into());
            args.push(f.clone());
        }
        if let Some(env) = &stack.env_file {
            args.push("--env-file".into());
            args.push(env.clone());
        }
        args
    }

    /// Runs `docker compose <args>` for a stack, streaming output to the task.
    pub async fn exec(&self, stack: &StackRef, args: &[&str], ctx: Option<&TaskCtx>) -> Result<String> {
        let mut full = self.base_args(stack);
        full.extend(args.iter().map(|s| s.to_string()));
        self.exec_raw(&full, ctx, Duration::from_secs(3600)).await
    }

    async fn exec_raw(&self, args: &[String], ctx: Option<&TaskCtx>, timeout: Duration) -> Result<String> {
        let cmd = self.command()?;
        let mut c = tokio::process::Command::new(&cmd[0]);
        c.args(&cmd[1..])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(ctx) = ctx {
            ctx.log(format!("$ docker compose {}", redact_args(args)));
        }
        let mut child = c.spawn().map_err(|e| AppError::Other(anyhow::anyhow!("failed to run compose: {e}")))?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let output = Arc::new(Mutex::new(Vec::<String>::new()));
        let pump = |reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
            let output = output.clone();
            let ctx = ctx.cloned();
            async move {
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ctx) = &ctx {
                        ctx.log(line.clone());
                    }
                    let mut out = output.lock();
                    out.push(line);
                    if out.len() > 2000 {
                        out.drain(..500);
                    }
                }
            }
        };
        let cancel = ctx.map(|c| c.cancel_token());
        let run = async {
            tokio::join!(pump(Box::new(stdout)), pump(Box::new(stderr)));
            child.wait().await
        };
        let status = tokio::select! {
            res = tokio::time::timeout(timeout, run) => match res {
                Ok(status) => status.map_err(|e| AppError::Other(e.into()))?,
                Err(_) => return Err(AppError::conflict("compose command timed out")),
            },
            _ = async { match &cancel { Some(t) => t.cancelled().await, None => std::future::pending().await } } => {
                return Err(AppError::conflict("cancelled"));
            }
        };
        let text = output.lock().join("\n");
        if status.success() {
            Ok(text)
        } else {
            let tail: Vec<&str> = text.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect();
            Err(AppError::conflict(format!(
                "compose exited with {}: {}",
                status.code().map(|c| c.to_string()).unwrap_or_else(|| "a signal".into()),
                tail.join("\n")
            )))
        }
    }

    /// The fully resolved project (`docker compose config`), cached until a
    /// file changes.
    pub async fn resolved(&self, stack: &StackRef) -> Result<Arc<ResolvedProject>> {
        let mut key = String::new();
        for f in stack.files.iter().map(PathBuf::from).chain(std::iter::once(stack.env_path())) {
            if let Ok(meta) = std::fs::metadata(&f) {
                use std::os::unix::fs::MetadataExt;
                key.push_str(&format!("{}:{}:{};", f.display(), meta.mtime_nsec() ^ meta.mtime(), meta.size()));
            }
        }
        if let Some((k, p)) = self.inner.config_cache.lock().get(&stack.name)
            && *k == key
        {
            return Ok(p.clone());
        }
        let profiles = self.exec(stack, &["config", "--profiles"], None).await.unwrap_or_default();
        let mut args: Vec<String> = Vec::new();
        for p in profiles.lines().map(str::trim).filter(|p| !p.is_empty()) {
            args.push("--profile".into());
            args.push(p.to_string());
        }
        let mut full = self.base_args(stack);
        full.extend(args);
        full.extend(["config".to_string(), "--format".to_string(), "json".to_string()]);
        let out = self.exec_raw(&full, None, Duration::from_secs(60)).await?;
        let json_start = out.find('{').ok_or_else(|| AppError::conflict("compose config produced no output"))?;
        let project: ResolvedProject = serde_json::from_str(&out[json_start..])
            .map_err(|e| AppError::Other(anyhow::anyhow!("could not read compose config: {e}")))?;
        let project = Arc::new(project);
        self.inner.config_cache.lock().insert(stack.name.clone(), (key, project.clone()));
        Ok(project)
    }

    /// Validates edited files by running `docker compose config` on copies.
    pub async fn validate(&self, stack: &StackRef, edits: &[FileEdit]) -> Result<std::result::Result<(), String>> {
        let tmp = self.inner.data_dir.join("tmp").join(format!("validate-{}", crate::util::short_id()));
        tokio::fs::create_dir_all(&tmp).await?;
        let result = async {
            let mut files = stack.files.clone();
            let mut env_file = stack.env_file.clone();
            for (i, edit) in edits.iter().enumerate() {
                let fname =
                    Path::new(&edit.path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                let tmp_path = tmp.join(format!("{i}-{fname}"));
                tokio::fs::write(&tmp_path, &edit.content).await?;
                let tmp_str = tmp_path.to_string_lossy().into_owned();
                if let Some(slot) = files.iter_mut().find(|f| **f == edit.path) {
                    *slot = tmp_str;
                } else if Path::new(&edit.path) == stack.env_path() {
                    env_file = Some(tmp_str);
                }
            }
            let temp_stack = StackRef { name: stack.name.clone(), dir: stack.dir.clone(), files, env_file };
            let mut args = self.base_args(&temp_stack);
            args.extend(["config".to_string(), "--quiet".to_string()]);
            Ok::<_, AppError>(match self.exec_raw(&args, None, Duration::from_secs(60)).await {
                Ok(_) => Ok(()),
                Err(AppError::Conflict(msg)) => Err(clean_validation_message(&msg, &tmp)),
                Err(e) => return Err(e),
            })
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        result
    }

    /// Compose and env files of a stack, for the editor.
    pub async fn read_files(&self, stack: &StackRef) -> Result<Vec<StackFile>> {
        let mut out = Vec::new();
        for f in &stack.files {
            out.push(StackFile {
                name: display_name(&stack.dir, f),
                path: f.clone(),
                kind: "compose",
                content: tokio::fs::read_to_string(f).await?,
                exists: true,
            });
        }
        let env = stack.env_path();
        let env_str = env.to_string_lossy().into_owned();
        let (content, exists) = match tokio::fs::read_to_string(&env).await {
            Ok(c) => (c, true),
            Err(_) => (String::new(), false),
        };
        out.push(StackFile { name: display_name(&stack.dir, &env_str), path: env_str, kind: "env", content, exists });
        Ok(out)
    }

    /// Checks that a write target belongs to the stack.
    pub fn is_stack_file(stack: &StackRef, path: &str) -> bool {
        stack.files.iter().any(|f| f == path) || Path::new(path) == stack.env_path()
    }

    /// Computes the file edits that point a service at a new tag.
    pub async fn plan_tag_change(&self, stack: &StackRef, service: &str, new_tag: &str) -> Result<Vec<FileChange>> {
        // The last file that defines the image wins (overrides come later).
        for file in stack.files.iter().rev() {
            let content = tokio::fs::read_to_string(file).await?;
            let Some(pos) = yaml_edit::find_service_image(&content, service).map_err(AppError::BadRequest)? else {
                continue;
            };
            let env_path = stack.env_path();
            let env_str = env_path.to_string_lossy().into_owned();
            let env_exists = tokio::fs::try_exists(&env_path).await.unwrap_or(false);
            let env_before = if env_exists { tokio::fs::read_to_string(&env_path).await? } else { String::new() };
            return match yaml_edit::plan_tag_edit(&pos.value, new_tag).map_err(AppError::BadRequest)? {
                yaml_edit::TagEdit::Literal(new_value) => Ok(vec![FileChange {
                    path: file.clone(),
                    after: yaml_edit::replace_scalar(&content, &pos, &new_value),
                    before: content,
                    created: false,
                }]),
                yaml_edit::TagEdit::EnvVar { name, value } => Ok(vec![FileChange {
                    path: env_str,
                    after: envfile::set(&env_before, &name, &value),
                    before: env_before,
                    created: !env_exists,
                }]),
                yaml_edit::TagEdit::EnvImage { name } => {
                    let default = yaml_edit::variables(&pos.value).first().and_then(|v| v.default.clone());
                    let current = envfile::get(&env_before, &name).or(default).ok_or_else(|| {
                        AppError::bad_request(format!(
                            "{name} is not set in {env_str}; set it before changing the version"
                        ))
                    })?;
                    let new_value = match yaml_edit::plan_tag_edit(&current, new_tag).map_err(AppError::BadRequest)? {
                        yaml_edit::TagEdit::Literal(v) => v,
                        _ => return Err(AppError::bad_request("nested variables are not supported")),
                    };
                    Ok(vec![FileChange {
                        path: env_str,
                        after: envfile::set(&env_before, &name, &new_value),
                        before: env_before,
                        created: !env_exists,
                    }])
                }
            };
        }
        Err(AppError::bad_request(format!(
            "service '{service}' has no image line Nodestarr can edit (it may be built locally or come from an anchor)"
        )))
    }

    /// Creates a new stack folder with the given files (not deployed yet).
    pub async fn create_stack(&self, name: &str, compose: &str, env: Option<&str>) -> Result<StackRef> {
        if !valid_stack_name(name) {
            return Err(AppError::bad_request(
                "stack names use lowercase letters, digits, '-' and '_' and start with a letter or digit",
            ));
        }
        if self.list().iter().any(|s| s.name == name) {
            return Err(AppError::conflict(format!("a stack named '{name}' already exists")));
        }
        let base = self
            .inner
            .settings
            .get()
            .docker
            .stacks_dirs
            .first()
            .cloned()
            .ok_or_else(|| AppError::bad_request("configure a stacks folder in Settings > Docker first"))?;
        let dir = Path::new(&base).join(name);
        if dir.exists() {
            return Err(AppError::conflict(format!("{} already exists", dir.display())));
        }
        tokio::fs::create_dir_all(&dir).await?;
        let compose_path = dir.join("compose.yaml");
        tokio::fs::write(&compose_path, compose).await?;
        if let Some(env) = env.filter(|e| !e.trim().is_empty()) {
            tokio::fs::write(dir.join(".env"), env).await?;
        }
        let stack = StackRef {
            name: name.to_string(),
            dir: dir.to_string_lossy().into_owned(),
            files: vec![compose_path.to_string_lossy().into_owned()],
            env_file: None,
        };
        if let Err(msg) = self.validate(&stack, &[]).await? {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return Err(AppError::BadRequest(msg));
        }
        Ok(stack)
    }

    /// Moves a stack folder into `<stacks dir>/.trash` (recoverable delete).
    pub async fn trash_stack_folder(&self, stack: &StackRef) -> Result<String> {
        let dir = PathBuf::from(&stack.dir);
        let parent = dir.parent().ok_or_else(|| AppError::bad_request("invalid stack folder"))?;
        let in_stacks_dir = self.inner.settings.get().docker.stacks_dirs.iter().any(|d| Path::new(d) == parent);
        if !in_stacks_dir {
            return Err(AppError::forbidden(
                "only stacks inside a configured stacks folder can be deleted from Nodestarr",
            ));
        }
        let trash = parent.join(".trash");
        tokio::fs::create_dir_all(&trash).await?;
        let target = trash.join(format!("{}-{}", stack.name, chrono::Utc::now().format("%Y%m%d-%H%M%S")));
        tokio::fs::rename(&dir, &target).await?;
        Ok(target.to_string_lossy().into_owned())
    }

    pub fn invalidate(&self, stack: &str) {
        self.inner.config_cache.lock().remove(stack);
    }
}

fn display_name(dir: &str, path: &str) -> String {
    Path::new(path).strip_prefix(dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
}

fn redact_args(args: &[String]) -> String {
    // Hide the global plumbing flags to keep task logs readable.
    let mut out = Vec::new();
    let mut skip = 0;
    for a in args {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if matches!(a.as_str(), "--ansi" | "--progress" | "--project-directory" | "-f" | "--env-file") {
            skip = 1;
            continue;
        }
        out.push(a.as_str());
    }
    out.join(" ")
}

fn clean_validation_message(msg: &str, tmp: &Path) -> String {
    let tmp = tmp.to_string_lossy();
    msg.replace(tmp.as_ref(), "")
        .trim_start_matches("compose exited with 1: ")
        .trim_start_matches("compose exited with 15: ")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names() {
        assert_eq!(normalize_project_name("My Stack.v2"), "mystackv2");
        assert_eq!(normalize_project_name("_media"), "media");
        assert!(valid_stack_name("media"));
        assert!(valid_stack_name("smart-home_2"));
        assert!(!valid_stack_name("Media"));
        assert!(!valid_stack_name("-x"));
        assert!(!valid_stack_name(""));
    }

    #[test]
    fn declared_names() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("compose.yaml");
        std::fs::write(&p, "name: Website\nservices:\n  web:\n    image: nginx\n").unwrap();
        assert_eq!(declared_name(&p).as_deref(), Some("website"));
        std::fs::write(&p, "services:\n  name:\n    image: nginx\n").unwrap();
        assert_eq!(declared_name(&p), None);
    }

    #[test]
    fn finds_files_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker-compose.yml"), "services: {}").unwrap();
        std::fs::write(dir.path().join("docker-compose.override.yml"), "services: {}").unwrap();
        let files = find_compose_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("docker-compose.yml"));
        assert!(files[1].ends_with("docker-compose.override.yml"));
    }

    #[test]
    fn args_redaction() {
        let args: Vec<String> = ["--ansi", "never", "--project-name", "media", "-f", "/x/compose.yaml", "up", "-d"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(redact_args(&args), "--project-name media up -d");
    }
}
