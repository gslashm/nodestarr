// Shapes of the JSON the Nodestarr API returns. Kept in one place so pages and
// stores agree; field names match the Rust structs (snake_case).

export type Severity = 'info' | 'success' | 'warning' | 'error';
export type UpdatePolicy = 'ignore' | 'notify' | 'auto';
export type TagScope = 'digest' | 'patch' | 'minor' | 'major';
/** How far an update moves the version. */
export type Bump = 'major' | 'minor' | 'patch' | 'rebuild' | 'unknown';
export type Role = 'admin' | 'viewer';

export interface PortInfo {
  private_port: number;
  public_port: number | null;
  protocol: string;
  host_ip: string | null;
}

export interface MountInfo {
  kind: string;
  source: string;
  destination: string;
  rw: boolean;
  name: string | null;
}

export interface WebLink {
  label: string;
  url: string;
}

export interface ContainerSummary {
  id: string;
  name: string;
  identity: string;
  stack: string | null;
  service: string | null;
  image: string;
  image_id: string;
  state: string;
  health: string | null;
  exit_code: number | null;
  oom_killed: boolean;
  created: number;
  started_at: number | null;
  finished_at: number | null;
  restart_count: number;
  restart_policy: string;
  ports: PortInfo[];
  networks: string[];
  network_mode: string;
  mounts: MountInfo[];
  version: string | null;
  version_source: string | null;
  app: string | null;
  app_name: string | null;
  links: WebLink[];
  compose_dir: string | null;
  compose_files: string[];
  is_self: boolean;
  label_name: string | null;
}

export type UpdateStatus =
  | 'unknown'
  | 'up_to_date'
  | 'available'
  | 'pending_recreate'
  | 'error'
  | 'ignored'
  | 'local'
  | 'pinned';

export interface NewerTag {
  tag: string;
  scope: TagScope;
  /** When Nodestarr first saw this tag (unix seconds). */
  first_seen: number | null;
}

export interface NotesFinding {
  kind: 'security' | 'breaking';
  /** Tag of the release it was found in. */
  release: string;
  /** The line of the release notes that says so. */
  text: string;
}

/** What an update's release notes said. */
export interface NotesScan {
  repo: string;
  /** Tags of the releases that were read, newest first. */
  releases: string[];
  /** Older releases in between weren't read (GitHub only gives the newest). */
  partial: boolean;
  findings: NotesFinding[];
  /** The new version's release page. */
  url: string | null;
}

/** What one update changes: its type. */
export interface Change {
  bump: Bump;
  /** The bump comes from the tag alone, so it's the most the update can be. */
  estimated: boolean;
  from_version: string | null;
  to_version: string | null;
  prerelease: boolean;
  security: boolean;
  breaking: boolean;
  notes: NotesScan | null;
}

/** Which updates automatic updates install, by type. */
export interface AutoInstall {
  rebuild: boolean;
  patch: boolean;
  minor: boolean;
  major: boolean;
  unknown: boolean;
  /** Security fixes, even when their version change isn't ticked (never major versions). */
  security: boolean;
  /** Updates whose release notes mention breaking changes. */
  breaking: boolean;
  /** Switch a version tag to a newer one. */
  switch_tags: boolean;
}

export interface UpdateInfo {
  status: UpdateStatus;
  image: string;
  checked_at: number | null;
  current_digest: string | null;
  latest_digest: string | null;
  latest_version: string | null;
  latest_created: number | null;
  first_seen: number | null;
  newer_tag: NewerTag | null;
  error: string | null;
  source: string | null;
  /** The offered update's type (the newer tag when there is one, else the new build). */
  change: Change | null;
  /** With a newer tag offered: what the new build of the current tag is. */
  build_change: Change | null;
}

export interface LiveStat {
  id: string;
  identity: string;
  cpu: number;
  mem: number;
  mem_limit: number;
  net_rx: number;
  net_tx: number;
  blk_read: number;
  blk_write: number;
  pids: number;
  ts: number;
}

export interface HealthIssue {
  level: 'notice' | 'warning' | 'error';
  message: string;
  source: string | null;
  wiki: string | null;
}

export type CredState = 'discovered' | 'configured' | 'missing' | 'not_needed';

export interface AppStatus {
  kind: string;
  reachable: boolean;
  version: string | null;
  branch: string | null;
  health: HealthIssue[];
  details: Record<string, string>;
  update_available: boolean | null;
  credentials: CredState;
  base_url: string | null;
  checked_at: number;
  error: string | null;
}

export interface PrefsBrief {
  tags: string[];
  icon: string;
  web_url: string;
  hidden: boolean;
  notes: boolean;
  held: boolean;
  skipped_digest: string;
  backup_enabled: boolean;
  alerts_muted: boolean;
  /** Dismissed "needs attention" items: kind to fingerprint, "*" for always. */
  dismissed: Record<string, string>;
}

export interface UptimeBrief {
  state: string;
  since: number;
  latency_ms: number | null;
  uptime_24h: number | null;
}

/** A container as the list endpoint returns it. */
export interface ContainerView extends ContainerSummary {
  display_name: string;
  policy: UpdatePolicy;
  /** Which updates install automatically (its own choice or the default). */
  auto_install: AutoInstall;
  /** Hours an update must be out before it installs automatically. */
  min_age_hours: number;
  update: UpdateInfo | null;
  prefs: PrefsBrief;
  stats: LiveStat | null;
  uptime: UptimeBrief | null;
  app_status: AppStatus | null;
  last_backup: number | null;
  busy: boolean;
}

export interface KeyValue {
  key: string;
  value: string;
  secret?: boolean;
}

export interface ContainerDetail {
  container: ContainerView;
  command: string[] | null;
  entrypoint: string[] | null;
  env: KeyValue[];
  labels: KeyValue[];
  networks: { name: string; ip: string | null; gateway: string | null; mac: string | null; aliases: string[] | null }[];
  hostname: string | null;
  user: string | null;
  working_dir: string | null;
  privileged: boolean;
  memory_limit: number | null;
  cpu_limit: number | null;
  devices: { host: string; container: string }[];
  health_log: { exit_code: number | null; output: string | null; end: string | null }[];
  image: {
    id: string;
    created: number | null;
    size: number;
    tags: string[];
    digests: string[];
    source: string | null;
    arch: string;
    os: string;
  } | null;
  notes: string;
  uptime: UptimeStatus | null;
  can_edit_spec: boolean;
}

export type TaskStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

export interface TaskSnapshot {
  id: string;
  kind: string;
  title: string;
  target: string | null;
  target_name: string | null;
  status: TaskStatus;
  actor: string | null;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
  progress: number | null;
  message: string | null;
  error: string | null;
  result: unknown;
}

export interface ActivityEntry {
  id: number;
  ts: number;
  kind: string;
  severity: Severity;
  target: string | null;
  target_name: string | null;
  message: string;
  actor: string | null;
  details: unknown;
}

export interface Beat {
  ts: number;
  ok: boolean;
  latency_ms: number;
}

export interface UptimeStatus {
  id: string;
  name: string;
  url: string;
  identity: string | null;
  state: 'up' | 'down' | 'pending' | 'paused' | string;
  since: number;
  last_check: number | null;
  latency_ms: number | null;
  status_code: number | null;
  error: string | null;
  uptime_24h: number | null;
  uptime_7d: number | null;
  beats: Beat[];
  custom: boolean;
}

export interface AlertView {
  id: number;
  rule_id: string;
  target: string;
  target_name: string | null;
  severity: Severity;
  message: string;
  started_at: number;
  resolved_at: number | null;
}

export interface HostStats {
  cpu: number;
  cpus: number;
  mem_total: number;
  mem_available: number;
  swap_total: number;
  swap_free: number;
  load: [number, number, number];
  uptime: number;
}

export interface DiskUsage {
  path: string;
  total: number;
  used: number;
  available: number;
}

export interface Series {
  ts: number[];
  cpu: number[];
  mem: number[];
  mem_limit: number[];
  net_rx: number[];
  net_tx: number[];
  blk_r: number[];
  blk_w: number[];
}

export interface StackContainer {
  id: string;
  name: string;
  identity: string;
  service: string | null;
  state: string;
  health: string | null;
}

export interface StackSummary {
  name: string;
  dir: string | null;
  files: string[];
  status: 'running' | 'partial' | 'stopped' | 'inactive';
  running: number;
  total: number;
  containers: StackContainer[];
  editable: boolean;
  in_stacks_dir: boolean;
  updates?: number;
}

export interface StackService {
  /** Null for containers that carry no service label. */
  name: string | null;
  image?: string | null;
  profiles?: string[];
  network_mode?: string | null;
  /** Built from a Dockerfile rather than pulled. */
  built?: boolean;
  container: StackContainer | null;
}

export interface StackDetail {
  stack: StackSummary;
  services: StackService[];
  /** Why the compose files couldn't be read, if they couldn't. */
  config_error: string | null;
}

export interface StackFile {
  name: string;
  path: string;
  kind: 'compose' | 'env';
  content: string;
  exists: boolean;
}

/** Any file of a stack folder, opened by the stack editor. */
export interface StackFileContent {
  path: string;
  name: string;
  content: string;
  exists: boolean;
  /** Why it can't be edited here (too large, not text). */
  error?: string;
}

export interface StackTreeEntry {
  /** Relative to the stack folder. */
  path: string;
  dir: boolean;
  size: number;
}

/** What an image sets up by itself. */
export interface ImageDefaults {
  /** `KEY=value` entries. */
  env: string[];
  /** Like "8989/tcp". */
  ports: string[];
  volumes: string[];
  workdir: string | null;
  user: string | null;
  cmd: string[] | null;
  entrypoint: string[] | null;
  healthcheck: string[] | null;
}

export interface ImageInspect {
  image: string;
  /** Read from this machine, or from the registry without pulling. */
  source: 'local' | 'registry';
  defaults: ImageDefaults;
  version: string | null;
  created: number | null;
  url: string | null;
  title: string | null;
  description: string | null;
}

export interface ImageTags {
  current: string;
  channels: TagInfo[];
  versions: TagInfo[];
  total: number;
}

export interface FileVersion {
  id: number;
  path: string;
  ts: number;
  actor: string | null;
  size: number;
  note: string | null;
}

export interface BackupRecord {
  id: string;
  identity: string;
  target_name: string | null;
  path: string;
  size: number;
  created_at: number;
  kind: string;
  status: string;
  pinned: boolean;
  manifest: BackupManifest;
  exists: boolean;
}

export interface BackupManifest {
  name?: string;
  image?: string;
  app_version?: string | null;
  method?: string;
  stack?: string | null;
  service?: string | null;
  container?: string;
  warnings?: string[];
  items?: { type: string; destination?: string; source?: string; path?: string; files?: number; bytes?: number; engine?: string; original?: string }[];
  excludes?: string[];
}

export interface PlannedMount {
  destination: string;
  source: string;
  kind: string;
  include: boolean;
  reason: string;
  access: 'direct' | 'docker_copy';
}

export interface BackupPlan {
  enabled: boolean;
  method: string;
  mounts: PlannedMount[];
  excludes: string[];
  db: { engine: string; description: string } | null;
  native: boolean;
  compose_files: string[];
  warnings: string[];
}

export interface HistoryEntry {
  id: number;
  identity: string;
  target_name: string | null;
  ts: number;
  from_ref: string | null;
  to_ref: string | null;
  from_version: string | null;
  to_version: string | null;
  rollback_tag: string | null;
  status: string;
  trigger: string | null;
  task_id: string | null;
  message: string | null;
  can_rollback: boolean;
}

export interface TagInfo {
  name: string;
  updated: number | null;
  digest: string | null;
}

export interface Release {
  tag: string;
  name: string;
  published_at: string | null;
  prerelease: boolean;
  url: string;
  body: string;
}

export interface UserInfo {
  id: number;
  username: string;
  role: Role;
  totp_enabled: boolean;
  created_at: number;
  last_login_at: number | null;
}

export interface ApiKeyInfo {
  id: number;
  name: string;
  prefix: string;
  role: Role;
  created_at: number;
  last_used_at: number | null;
}

export interface SessionInfo {
  id: string;
  created_at: number;
  last_seen_at: number;
  expires_at: number;
  ip: string | null;
  user_agent: string | null;
  current: boolean;
}

export interface AuthStatus {
  authenticated: boolean;
  user: { username: string; role: Role; via: string } | null;
  setup_required: boolean;
  mfa_pending: boolean;
  auth_mode: string;
  instance_name: string;
  version: string;
}

export interface Me {
  username: string;
  role: Role;
  via: string;
  totp_enabled: boolean;
  can_change_password: boolean;
}

export interface ImageRow {
  id: string;
  tags: string[];
  digests: string[];
  size: number;
  created: number;
  containers: string[];
  dangling: boolean;
  rollback: boolean;
}

export interface VolumeRow {
  name: string;
  driver: string;
  mountpoint: string;
  created: number | null;
  stack: string | null;
  anonymous: boolean;
  containers: string[];
  size: number | null;
  labels: Record<string, string>;
}

export interface NetworkRow {
  id: string;
  name: string;
  driver: string;
  scope: string;
  internal: boolean;
  ipv6: boolean;
  subnets: string[];
  gateways: string[];
  stack: string | null;
  containers: string[];
  builtin: boolean;
  created: number | null;
}

export interface UsageGroup {
  count: number;
  active: number;
  size: number;
  reclaimable: number;
}

export interface DockerInfo {
  version: string;
  api_version: string;
  os: string;
  kernel: string;
  arch: string;
  cpus: number;
  memory: number;
  storage_driver: string;
  root_dir: string;
  containers: number;
  images: number;
  compose_version: string | null;
}

export interface IntegrationApp {
  identity: string;
  id: string;
  name: string;
  app: string | null;
  kind: string | null;
  running: boolean;
  enabled: boolean;
  overridden: boolean;
  status: AppStatus | null;
}

// --------------------------------------------------------------- Proxmox

export interface PveNode {
  node: string;
  cpu: number;
  cpus: number;
  cpu_model: string;
  mem_used: number;
  mem_total: number;
  swap_used: number;
  swap_total: number;
  rootfs_used: number;
  rootfs_total: number;
  loadavg: string[];
  uptime: number;
  pve_version: string;
  kernel: string;
  ksm_shared: number;
}

/** A cluster member, from Proxmox's cluster resources. */
export interface PveNodeSummary {
  node: string;
  online: boolean;
  cpu: number;
  cpus: number;
  mem: number;
  maxmem: number;
}

export interface PveGuest {
  vmid: number;
  name: string;
  kind: 'lxc' | 'qemu' | string;
  node: string;
  status: string;
  cpu: number;
  cpus: number;
  mem: number;
  maxmem: number;
  disk: number;
  maxdisk: number;
  netin: number;
  netout: number;
  uptime: number;
  tags: string[];
  template: boolean;
  is_self: boolean;
}

export interface PveStorage {
  storage: string;
  kind: string;
  content: string[];
  used: number;
  total: number;
  active: boolean;
  shared: boolean;
}

export interface PveOverview {
  node: PveNode;
  nodes: PveNodeSummary[];
  guests: PveGuest[];
  storages: PveStorage[];
  fetched_at: number;
}

export interface PveSnapshot {
  name: string;
  description: string;
  snaptime: number | null;
  parent: string | null;
  vmstate: number | null;
}

export interface PveBackup {
  volid: string;
  ctime: number | null;
  size: number | null;
  vmid: number | null;
  notes: string | null;
  format: string | null;
  protected: number | null;
}

export interface PveTask {
  upid: string;
  /** Task type, like "vzdump", "qmstart" or "vzsnapshot". */
  type: string;
  /** "OK", an error message, or null while running. */
  status: string | null;
  starttime: number | null;
  endtime: number | null;
  user: string | null;
  /** Usually the guest the task is about. */
  id: string | null;
}

/** A package update on the node (field names as Proxmox sends them). */
export interface PveAptUpdate {
  Package: string;
  Title: string;
  OldVersion: string;
  Version: string;
  Priority: string;
}

export interface PveDisk {
  devpath: string;
  model: string | null;
  serial: string | null;
  size: number | null;
  health: string | null;
  /** Percent of the rated endurance used up (SSDs), or "N/A". */
  wearout: number | string | null;
  type: string | null;
  used: string | null;
}

// ------------------------------------------------- Proxmox disk health

/** How much a finding matters; "info" never changes a disk's verdict. */
export type DiskLevel = 'ok' | 'info' | 'watch' | 'warning' | 'failing';
export type DiskMedia = 'hdd' | 'ssd' | 'nvme' | 'unknown';
export type TestOutcome = 'passed' | 'failed' | 'aborted' | 'interrupted' | 'running' | 'unknown';
export type PoolKind = 'zfs' | 'md' | 'snapraid' | 'lvmthin';
export type PoolHealth = 'ok' | 'degraded' | 'faulted' | 'unknown';

export interface DiskFinding {
  level: DiskLevel;
  code: string;
  title: string;
  detail: string;
}

export interface DiskRole {
  pool: string;
  pool_name: string;
  kind: PoolKind;
  /** data, parity, spare, log, cache, special, dedup. */
  role: string;
  group: string;
  /** Its name in the pool ("d1" and "parity 2" in SnapRAID). */
  member: string;
  state: string | null;
  ok: boolean;
}

export interface DiskLabel {
  bay: string;
  notes: string;
  purchased: string;
  warranty_until: string;
}

export interface DiskTestSupport {
  short: boolean;
  long: boolean;
  conveyance: boolean;
  short_minutes: number | null;
  long_minutes: number | null;
  conveyance_minutes: number | null;
  available: boolean;
  reason: string | null;
}

export interface DiskLastTest {
  kind: string;
  outcome: TestOutcome;
  status: string;
  at: number | null;
  /** The time is estimated from the drive's power-on hours. */
  estimated: boolean;
  lba: number | null;
}

export interface DiskView {
  id: string;
  node: string;
  devpath: string;
  by_id: string | null;
  model: string | null;
  family: string | null;
  serial: string | null;
  wwn: string | null;
  vendor: string | null;
  firmware: string | null;
  media: DiskMedia;
  transport: string | null;
  size: number | null;
  rpm: number | null;
  used_by: string | null;
  osd: number | null;
  roles: DiskRole[];
  mounts: string[];
  present: boolean;
  first_seen: number;
  last_seen: number;
  sampled_at: number | null;
  checked_at: number | null;
  source: 'api' | 'helper' | null;
  asleep: boolean;
  /** Spins down between readings, so it's read through the Proxmox API only about once a day. */
  resting: boolean;
  error: string | null;
  passed: boolean | null;
  verdict: DiskLevel;
  findings: DiskFinding[];
  temperature: number | null;
  temp_limit: number | null;
  power_on_hours: number | null;
  power_cycles: number | null;
  reallocated: number | null;
  pending: number | null;
  uncorrectable: number | null;
  crc_errors: number | null;
  wear_used: number | null;
  bytes_written: number | null;
  load_cycles: number | null;
  tests: DiskTestSupport;
  last_test: DiskLastTest | null;
  label: DiskLabel;
  smr: boolean;
  /** Hourly temperatures of the last day, oldest first. */
  temps: (number | null)[];
}

export interface PoolMember {
  name: string;
  path: string | null;
  disk: string | null;
  role: string;
  state: string | null;
  ok: boolean;
  read: number | null;
  write: number | null;
  cksum: number | null;
  note: string | null;
  slot: number;
  size: number | null;
  used: number | null;
  level: number | null;
}

export interface PoolGroup {
  name: string;
  /** data, log, cache, spare, special, dedup, parity. */
  class: string;
  kind: string;
  parity: number;
  state: string | null;
  members: PoolMember[];
}

export interface PoolScan {
  kind: string;
  /** running, paused, finished, canceled, pending, none. */
  state: string;
  progress: number | null;
  eta_secs: number | null;
  speed: string | null;
  started_at: number | null;
  finished_at: number | null;
  duration_secs: number | null;
  repaired: string | null;
  errors: number | null;
  text: string;
}

export interface PoolTolerance {
  designed: number;
  failed: number;
  remaining: number;
}

export interface SnapraidInfo {
  parity_levels: number;
  content_files: string[];
  last_sync: number | null;
  status_at: number | null;
  oldest_scrub_days: number | null;
  median_scrub_days: number | null;
  newest_scrub_days: number | null;
  not_scrubbed_pct: number | null;
  sync_progress: number | null;
  errors: number | null;
  busy: boolean;
  summary: string[];
}

export interface Pool {
  id: string;
  node: string;
  kind: PoolKind;
  name: string;
  state: string;
  health: PoolHealth;
  layout: string;
  size: number | null;
  used: number | null;
  free: number | null;
  fragmentation: number | null;
  dedup: number | null;
  groups: PoolGroup[];
  scan: PoolScan | null;
  tolerance: PoolTolerance | null;
  status_text: string | null;
  action: string | null;
  errors_text: string | null;
  data_errors: number | null;
  metadata_size: number | null;
  metadata_used: number | null;
  snapraid: SnapraidInfo | null;
  findings: DiskFinding[];
  growth_per_day: number | null;
  full_in_days: number | null;
}

export interface DiskTest {
  id: string;
  disk: string;
  node: string;
  devpath: string;
  kind: 'short' | 'long' | 'conveyance' | string;
  /** queued, running, passed, failed, aborted, interrupted, skipped, error. */
  status: string;
  queued_at: number;
  started_at: number | null;
  finished_at: number | null;
  progress: number | null;
  expected_secs: number | null;
  start_hours: number | null;
  result: string | null;
  lba: number | null;
  message: string | null;
  actor: string | null;
  schedule: string | null;
}

export interface TestSchedule {
  id: string;
  name: string;
  enabled: boolean;
  kind: 'short' | 'long' | 'conveyance';
  cron: string;
  disks: 'all' | 'hdd' | 'ssd' | 'nvme' | 'list';
  ids: string[];
  nodes: string[];
  /** Filled in by the server. */
  next_run?: number | null;
  covers?: number;
}

export interface DiskEvent {
  id: number;
  subject: string;
  ts: number;
  severity: Severity;
  kind: string;
  message: string;
}

export interface NodeAccess {
  node: string;
  address: string;
  ok: boolean;
  checked_at: number | null;
  error: string | null;
  helper: number | null;
  outdated: boolean;
  hostname: string | null;
  smartctl: string | null;
  zfs: string | null;
  mdadm: boolean;
  md_arrays: boolean;
  snapraid: string | null;
  snapraid_conf: boolean;
  host_keys: string[];
}

export interface DiskSampling {
  running: boolean;
  last_run: number | null;
  last_smart: number | null;
  next_smart: number | null;
  error: string | null;
  node_errors: Record<string, string>;
}

export interface DiskHealthOverview {
  enabled: boolean;
  proxmox: boolean;
  sampling: DiskSampling;
  sample_minutes: number;
  host_access: { enabled: boolean; has_key: boolean; nodes: NodeAccess[] };
  disks: DiskView[];
  pools: Pool[];
  tests: { active: DiskTest[]; recent: DiskTest[] };
  schedules: TestSchedule[];
  events: DiskEvent[];
}

export interface SmartAttribute {
  id: number;
  name: string;
  value: number | null;
  worst: number | null;
  thresh: number | null;
  raw: string;
  prefail: boolean;
  /** "" (never), "now" or "past". */
  failed: string;
}

export interface SmartSelfTestEntry {
  kind: string;
  status: string;
  outcome: TestOutcome;
  hours: number | null;
  lba: number | null;
  remaining: number | null;
}

export interface SmartReport {
  source: 'api' | 'helper';
  protocol: 'ata' | 'nvme' | 'scsi' | 'unknown';
  collected_at: number;
  model: string | null;
  family: string | null;
  serial: string | null;
  firmware: string | null;
  capacity: number | null;
  logical_sector: number | null;
  physical_sector: number | null;
  rpm: number | null;
  form_factor: string | null;
  link_max: string | null;
  link_now: string | null;
  trim: boolean | null;
  erc_read: number | null;
  erc_write: number | null;
  smart_available: boolean | null;
  smart_enabled: boolean | null;
  passed: boolean | null;
  temperature: number | null;
  temp_min_lifetime: number | null;
  temp_max_lifetime: number | null;
  temp_warning: number | null;
  temp_critical: number | null;
  power_on_hours: number | null;
  power_cycles: number | null;
  reallocated: number | null;
  reallocated_events: number | null;
  pending: number | null;
  offline_uncorrectable: number | null;
  reported_uncorrect: number | null;
  crc_errors: number | null;
  command_timeouts: number | null;
  spin_retries: number | null;
  load_cycles: number | null;
  helium: number | null;
  wear_used: number | null;
  available_spare: number | null;
  spare_threshold: number | null;
  critical_warning: number | null;
  media_errors: number | null;
  error_log_entries: number | null;
  unsafe_shutdowns: number | null;
  bytes_written: number | null;
  bytes_read: number | null;
  grown_defects: number | null;
  uncorrected_errors: number | null;
  attributes: SmartAttribute[];
  self_tests: {
    short: boolean;
    long: boolean;
    conveyance: boolean;
    short_minutes: number | null;
    long_minutes: number | null;
    conveyance_minutes: number | null;
    running: { kind: string | null; remaining: number | null } | null;
    log: SmartSelfTestEntry[];
  };
  messages: string[];
  text: string | null;
}

export interface DiskDetail {
  disk: DiskView;
  report: SmartReport | null;
  has_raw: boolean;
  test: DiskTest | null;
  tests: DiskTest[];
  events: DiskEvent[];
}

export interface DiskHistoryPoint {
  ts: number;
  temp: number | null;
  temp_max: number | null;
  poh: number | null;
  realloc: number | null;
  pending: number | null;
  uncorrect: number | null;
  crc: number | null;
  wear: number | null;
  written: number | null;
  cycles: number | null;
  /** Share of readings in the bucket that found the disk asleep (0..1). */
  asleep: number;
}

export interface HostAccessNode {
  node: string;
  address: string | null;
  configured: boolean;
  status: NodeAccess | null;
  host_keys: string[];
}

export interface HostAccessInfo {
  enabled: boolean;
  has_key: boolean;
  public_key: string | null;
  install: string | null;
  helper_version: number;
  helper_path: string;
  nodes: HostAccessNode[];
}

// -------------------------------------------------------------- settings

export interface NotificationProvider {
  id: string;
  name: string;
  enabled: boolean;
  events: string[];
  min_severity: Severity;
  config: ProviderConfig;
}

export type ProviderConfig =
  | { kind: 'discord'; webhook_url: string; username: string; mention: string }
  | { kind: 'email'; host: string; port: number; security: string; username: string; password: string; from: string; to: string[] }
  | { kind: 'telegram'; bot_token: string; chat_id: string; topic_id: string }
  | { kind: 'ntfy'; server: string; topic: string; token: string; username: string; password: string }
  | { kind: 'gotify'; server: string; app_token: string }
  | { kind: 'pushover'; user_key: string; app_token: string; device: string }
  | { kind: 'slack'; webhook_url: string }
  | { kind: 'webhook'; url: string; method: string; headers: Record<string, string>; body_template: string };

export interface AlertRule {
  id: string;
  kind: string;
  enabled: boolean;
  threshold: number;
  duration_secs: number;
  severity: Severity;
}

export interface CustomCheck {
  id: string;
  name: string;
  url: string;
  expect_status: string;
  keyword: string;
  enabled: boolean;
}

export interface Settings {
  general: {
    instance_name: string;
    url_base: string;
    public_host: string;
    external_url: string;
    timezone: string;
    ui: {
      theme: string;
      accent: string;
      density: string;
      icon_url_template: string;
      group_by_stack: boolean;
      time_format: string;
    };
    check_nodestarr_updates: boolean;
  };
  security: {
    auth_mode: 'forms' | 'external';
    bypass_local: boolean;
    local_networks: string[];
    trusted_proxies: string[];
    external_header: string;
    session_hours: number;
    remember_days: number;
    allow_query_api_key: boolean;
    metrics_public: boolean;
  };
  docker: {
    stacks_dirs: string[];
    compose_command: string;
    stop_timeout_secs: number;
    hidden_containers: string[];
    console_shells: string[];
    resync_secs: number;
  };
  updates: {
    check_cron: string;
    default_policy: UpdatePolicy;
    auto_install: AutoInstall;
    auto_cron: string;
    min_age_hours: number;
    notify_scope: TagScope;
    ignore_prereleases: boolean;
    settle_secs: number;
    verify_timeout_secs: number;
    rollback_on_failure: boolean;
    backup_before_update: boolean;
    keep_rollback_images: number;
    prune_after_update: boolean;
    plex_guard: boolean;
    notify_available: boolean;
    registries: { registry: string; username: string; password: string }[];
    github_token: string;
  };
  backups: {
    root: string;
    schedule_enabled: boolean;
    cron: string;
    include_all_by_default: boolean;
    retention_count: number;
    retention_days: number;
    pre_update_retention_days: number;
    compression: 'zstd' | 'gzip';
    zstd_level: number;
    default_strategy: 'auto' | 'pause' | 'stop' | 'live';
    include_compose: boolean;
    data_roots: string[];
    exclude: string[];
    hook: { command: string; exec_container: string; exec_command: string; when: 'success' | 'always'; timeout_secs: number };
  };
  notifications: {
    providers: NotificationProvider[];
    quiet_hours: { enabled: boolean; start: string; end: string; allow_errors: boolean };
    dedup_minutes: number;
  };
  integrations: {
    enabled: boolean;
    poll_secs: number;
    auto_discover: boolean;
    qbittorrent_username: string;
    qbittorrent_password: string;
  };
  proxmox: {
    enabled: boolean;
    url: string;
    token_id: string;
    token_secret: string;
    tls_mode: 'fingerprint' | 'verify' | 'insecure';
    fingerprint: string;
    node: string;
    self_vmid: number | null;
    backup_storage: string;
    disks: {
      enabled: boolean;
      sample_minutes: number;
      wake_sleeping: boolean;
      retention_days: number;
      max_parallel_tests: number;
      notify_passed: boolean;
      schedules: TestSchedule[];
    };
    host_access: {
      enabled: boolean;
      user: string;
      port: number;
      addresses: Record<string, string>;
    };
  };
  mqtt: {
    enabled: boolean;
    host: string;
    port: number;
    tls: boolean;
    username: string;
    password: string;
    client_id: string;
    discovery_prefix: string;
    base_topic: string;
    expose_stats: boolean;
    expose_controls: boolean;
  };
  monitoring: {
    history_enabled: boolean;
    sample_secs: number;
    raw_retention_hours: number;
    rollup_retention_days: number;
    live_secs: number;
    disk_paths: string[];
    rules: AlertRule[];
    activity_retention_days: number;
    task_retention_days: number;
  };
  uptime: {
    enabled: boolean;
    interval_secs: number;
    timeout_secs: number;
    failures_before_down: number;
    auto_checks: boolean;
    expect_status: string;
    custom: CustomCheck[];
    retention_days: number;
  };
}

export interface ContainerPrefs {
  display_name: string;
  icon: string;
  notes: string;
  tags: string[];
  hidden: boolean;
  web_url: string;
  alerts_muted: boolean;
  dismissed: Record<string, string>;
  update: {
    policy: UpdatePolicy | null;
    auto_install: AutoInstall | null;
    skipped_digest: string;
    held: boolean;
    min_age_hours: number | null;
  };
  backup: {
    enabled: boolean | null;
    strategy: 'auto' | 'pause' | 'stop' | 'live' | null;
    mounts: { destination: string; include: boolean }[];
    exclude: string[];
    use_native: boolean | null;
    retention_count: number | null;
  };
  uptime: { enabled: boolean | null; url: string; expect_status: string; keyword: string };
  integration: { enabled: boolean | null; base_url: string; api_key: string; username: string; password: string };
}

export interface PortSpec {
  host_ip: string;
  host_port: string;
  container_port: string;
  protocol: string;
}

export interface VolumeSpec {
  kind: string;
  source: string;
  target: string;
  read_only: boolean;
}

export interface ContainerSpec {
  name: string;
  image: string;
  restart: string;
  network_mode: string;
  networks: string[];
  ports: PortSpec[];
  volumes: VolumeSpec[];
  env: KeyValue[];
  labels: KeyValue[];
  devices: { host: string; container: string; permissions: string }[];
  group_add: string[];
  cap_add: string[];
  cap_drop: string[];
  privileged: boolean;
  user: string;
  hostname: string;
  working_dir: string;
  command: string[];
  entrypoint: string[];
  memory: number | null;
  cpus: number | null;
  shm_size: number | null;
  extra_hosts: string[];
  dns: string[];
  gpus: string;
  tty: boolean;
  stdin_open: boolean;
  init: boolean;
  healthcheck: { command: string; interval_secs: number; timeout_secs: number; retries: number; start_period_secs: number } | null;
  log_driver: string;
  log_opts: KeyValue[];
  security_opt: string[];
  sysctls: KeyValue[];
  tmpfs: string[];
}

export interface LogLine {
  ts?: string;
  s: 'out' | 'err' | 'tty';
  t: string;
  /** Service name, in stack logs. */
  c?: string;
}
