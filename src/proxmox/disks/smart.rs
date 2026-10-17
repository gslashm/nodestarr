//! S.M.A.R.T. reports: one model for what `smartctl -j -x` (through the host
//! helper) and the Proxmox API (`disks/smart`) report, and an assessment of
//! what the numbers mean for the disk.
//!
//! Everything here is pure: parsing never fails (unknown or missing fields
//! simply stay `None`), so a quirky drive degrades to "less detail", never
//! to an error.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Unknown,
    Ata,
    Nvme,
    Scsi,
}

/// Where a report came from: the helper's smartctl JSON is complete, the
/// Proxmox API only has health, attributes and (NVMe, SAS) a text dump.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Api,
    Helper,
}

/// What a disk is made of, as far as health rules care.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Hdd,
    Ssd,
    Nvme,
    #[default]
    Unknown,
}

impl Media {
    pub fn is_flash(self) -> bool {
        matches!(self, Media::Ssd | Media::Nvme)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attribute {
    pub id: u16,
    pub name: String,
    /// Normalized value, worst value and threshold (higher is better).
    pub value: Option<u16>,
    pub worst: Option<u16>,
    pub thresh: Option<u16>,
    pub raw: String,
    /// Pre-failure attribute (crossing its threshold predicts failure).
    pub prefail: bool,
    /// "" (never), "now" or "past".
    pub failed: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome {
    Passed,
    Failed,
    /// Stopped on purpose (by the host or a command).
    Aborted,
    /// Cut short by a reset, power loss or the disk going to sleep.
    Interrupted,
    Running,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelfTestEntry {
    /// "short", "long", "conveyance", "offline", "selective" or "vendor".
    pub kind: String,
    /// The drive's own wording, e.g. "Completed: read failure".
    pub status: String,
    pub outcome: TestOutcome,
    /// Power-on hours when the test ended.
    pub hours: Option<u64>,
    /// First failing block, for failed tests.
    pub lba: Option<u64>,
    pub remaining: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunningTest {
    pub kind: Option<String>,
    /// Percent of the test still to do.
    pub remaining: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelfTests {
    pub short: bool,
    pub long: bool,
    pub conveyance: bool,
    /// The drive's own estimates, in minutes.
    pub short_minutes: Option<u32>,
    pub long_minutes: Option<u32>,
    pub conveyance_minutes: Option<u32>,
    pub running: Option<RunningTest>,
    /// The drive's self-test log, newest first.
    pub log: Vec<SelfTestEntry>,
}

impl SelfTests {
    pub fn supports(&self, kind: &str) -> bool {
        match kind {
            "short" => self.short,
            "long" => self.long,
            "conveyance" => self.conveyance,
            _ => false,
        }
    }

    pub fn minutes(&self, kind: &str) -> Option<u32> {
        match kind {
            "short" => self.short_minutes,
            "long" => self.long_minutes,
            "conveyance" => self.conveyance_minutes,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub source: Source,
    pub protocol: Protocol,
    pub collected_at: i64,
    pub model: Option<String>,
    pub family: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub capacity: Option<u64>,
    pub logical_sector: Option<u32>,
    pub physical_sector: Option<u32>,
    /// Spindle speed; 0 for solid state.
    pub rpm: Option<u32>,
    pub form_factor: Option<String>,
    /// Interface speed the drive supports and the one negotiated, e.g. "6.0 Gb/s".
    pub link_max: Option<String>,
    pub link_now: Option<String>,
    pub trim: Option<bool>,
    /// SCT error recovery control in tenths of a second; Some(0) when off.
    pub erc_read: Option<u32>,
    pub erc_write: Option<u32>,
    pub smart_available: Option<bool>,
    pub smart_enabled: Option<bool>,
    /// The drive's overall verdict ("PASSED").
    pub passed: Option<bool>,
    pub temperature: Option<f64>,
    pub temp_min_lifetime: Option<f64>,
    pub temp_max_lifetime: Option<f64>,
    /// The drive's own limits: recommended maximum and absolute maximum.
    pub temp_warning: Option<f64>,
    pub temp_critical: Option<f64>,
    pub power_on_hours: Option<u64>,
    pub power_cycles: Option<u64>,
    pub reallocated: Option<u64>,
    pub reallocated_events: Option<u64>,
    pub pending: Option<u64>,
    pub offline_uncorrectable: Option<u64>,
    pub reported_uncorrect: Option<u64>,
    pub crc_errors: Option<u64>,
    pub command_timeouts: Option<u64>,
    pub spin_retries: Option<u64>,
    pub load_cycles: Option<u64>,
    /// Times the spindle started (ATA Start_Stop_Count, SAS start-stop
    /// cycles): it grows each time a sleeping disk wakes up.
    pub start_stops: Option<u64>,
    /// Normalized helium level (100 = full) on helium-filled drives.
    pub helium: Option<u16>,
    /// Percent of the rated write endurance used (can exceed 100).
    pub wear_used: Option<f64>,
    pub available_spare: Option<u32>,
    pub spare_threshold: Option<u32>,
    pub critical_warning: Option<u32>,
    pub media_errors: Option<u64>,
    pub error_log_entries: Option<u64>,
    pub unsafe_shutdowns: Option<u64>,
    pub bytes_written: Option<u64>,
    pub bytes_read: Option<u64>,
    /// SAS: elements in the grown defect list, and uncorrected errors.
    pub grown_defects: Option<u64>,
    pub uncorrected_errors: Option<u64>,
    pub attributes: Vec<Attribute>,
    pub self_tests: SelfTests,
    /// Warnings smartctl printed while reading the disk.
    pub messages: Vec<String>,
    /// The Proxmox API's text dump (NVMe and SAS disks).
    pub text: Option<String>,
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// The first whole number in `s`, accepting thousands separators: "41234h+5m"
/// → 41234, "34 (Min/Max 20/45)" → 34, "12,345,678 [6.32 TB]" → 12345678.
pub fn first_uint(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(u8::is_ascii_digit)?;
    let mut n: u64 = 0;
    let mut i = start;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            n = n.checked_mul(10)?.checked_add(u64::from(bytes[i] - b'0'))?;
            i += 1;
        }
        // "12,345": a comma followed by exactly three digits continues the number.
        let group = i + 4 <= bytes.len()
            && bytes[i] == b','
            && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit)
            && bytes.get(i + 4).is_none_or(|b| !b.is_ascii_digit());
        if group {
            i += 1;
        } else {
            return Some(n);
        }
    }
}

fn num_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => first_uint(s),
        _ => None,
    }
}

fn num_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok().or_else(|| first_uint(s).map(|n| n as f64)),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn u32_of(v: &Value) -> Option<u32> {
    num_u64(v).and_then(|n| u32::try_from(n).ok())
}

// ---------------------------------------------------------------------------
// smartctl JSON (the host helper runs `smartctl -j -x`)
// ---------------------------------------------------------------------------

/// Builds a report from `smartctl -j -x` output.
pub fn from_smartctl(v: &Value, now: i64) -> Report {
    let mut r = Report { source: Source::Helper, collected_at: now, ..Report::default() };
    r.protocol = match v["device"]["protocol"].as_str().unwrap_or("") {
        "ATA" => Protocol::Ata,
        "NVMe" => Protocol::Nvme,
        "SCSI" => Protocol::Scsi,
        _ => Protocol::Unknown,
    };
    r.model = text(&v["model_name"]).or_else(|| text(&v["scsi_model_name"])).or_else(|| {
        let vendor = text(&v["scsi_vendor"]).unwrap_or_default();
        text(&v["scsi_product"]).map(|p| format!("{vendor} {p}").trim().to_string())
    });
    r.family = text(&v["model_family"]);
    r.serial = text(&v["serial_number"]);
    r.firmware = text(&v["firmware_version"]).or_else(|| text(&v["scsi_revision"]));
    r.capacity = num_u64(&v["user_capacity"]["bytes"]).or_else(|| num_u64(&v["nvme_total_capacity"]));
    r.logical_sector = u32_of(&v["logical_block_size"]);
    r.physical_sector = u32_of(&v["physical_block_size"]);
    r.rpm = u32_of(&v["rotation_rate"]);
    r.form_factor = text(&v["form_factor"]["name"]);
    r.link_max = text(&v["interface_speed"]["max"]["string"]);
    r.link_now = text(&v["interface_speed"]["current"]["string"]);
    r.trim = v["trim"]["supported"].as_bool();
    if let Some(erc) = v.get("ata_sct_erc") {
        let side = |s: &Value| match s["enabled"].as_bool() {
            Some(false) => Some(0),
            _ => u32_of(&s["deciseconds"]),
        };
        r.erc_read = side(&erc["read"]);
        r.erc_write = side(&erc["write"]);
    }
    r.smart_available = v["smart_support"]["available"].as_bool();
    r.smart_enabled = v["smart_support"]["enabled"].as_bool();
    r.passed = v["smart_status"]["passed"].as_bool();

    let t = &v["temperature"];
    r.temperature = num_f64(&t["current"]);
    r.temp_min_lifetime = num_f64(&t["lifetime_min"]);
    r.temp_max_lifetime = num_f64(&t["lifetime_max"]);
    r.temp_warning = num_f64(&t["op_limit_max"]);
    r.temp_critical =
        num_f64(&t["critical_limit_max"]).or_else(|| num_f64(&t["limit_max"])).or_else(|| num_f64(&t["drive_trip"]));
    r.power_on_hours = num_u64(&v["power_on_time"]["hours"]);
    r.power_cycles = num_u64(&v["power_cycle_count"]);

    if let Some(msgs) = v["smartctl"]["messages"].as_array() {
        r.messages = msgs.iter().filter_map(|m| text(&m["string"])).collect();
    }

    if let Some(table) = v["ata_smart_attributes"]["table"].as_array() {
        r.attributes = table
            .iter()
            .filter_map(|a| {
                let id = u16::try_from(num_u64(&a["id"])?).ok()?;
                let failed = match a["when_failed"].as_str().unwrap_or("") {
                    "now" => "now",
                    "past" => "past",
                    _ => "",
                };
                Some(Attribute {
                    id,
                    name: text(&a["name"]).unwrap_or_else(|| format!("Attribute_{id}")),
                    value: num_u64(&a["value"]).and_then(|n| u16::try_from(n).ok()),
                    worst: num_u64(&a["worst"]).and_then(|n| u16::try_from(n).ok()),
                    thresh: num_u64(&a["thresh"]).and_then(|n| u16::try_from(n).ok()),
                    raw: text(&a["raw"]["string"])
                        .or_else(|| num_u64(&a["raw"]["value"]).map(|n| n.to_string()))
                        .unwrap_or_default(),
                    prefail: a["flags"]["prefailure"].as_bool().unwrap_or(false),
                    failed: failed.to_string(),
                })
            })
            .collect();
    }
    let sector = u64::from(r.logical_sector.unwrap_or(512));
    apply_attributes(&mut r, sector);
    apply_device_statistics(&mut r, &v["ata_device_statistics"], sector);
    if let Some(errors) = num_u64(&v["ata_smart_error_log"]["extended"]["count"])
        .or_else(|| num_u64(&v["ata_smart_error_log"]["summary"]["count"]))
    {
        r.error_log_entries = Some(errors);
    }
    if let Some(table) = v["sata_phy_event_counters"]["table"].as_array() {
        // The phy counters see CRC errors even on drives without attribute 199.
        let crc = table
            .iter()
            .filter(|c| c["name"].as_str().is_some_and(|n| n.contains("CRC error")))
            .filter_map(|c| num_u64(&c["value"]))
            .sum::<u64>();
        if r.crc_errors.is_none() && table.iter().any(|c| c["name"].as_str().is_some_and(|n| n.contains("CRC error"))) {
            r.crc_errors = Some(crc);
        }
    }

    match r.protocol {
        Protocol::Ata => ata_self_tests(&mut r, v),
        Protocol::Nvme => nvme(&mut r, v),
        Protocol::Scsi => scsi(&mut r, v),
        Protocol::Unknown => {}
    }
    if r.temperature.is_none() {
        r.temperature = attribute_temperature(&r.attributes);
    }
    r
}

fn ata_self_tests(r: &mut Report, v: &Value) {
    let data = &v["ata_smart_data"];
    let caps = &data["capabilities"];
    let st = &mut r.self_tests;
    st.short = caps["self_tests_supported"].as_bool().unwrap_or(false);
    st.long = st.short;
    st.conveyance = caps["conveyance_self_test_supported"].as_bool().unwrap_or(false);
    let poll = &data["self_test"]["polling_minutes"];
    st.short_minutes = u32_of(&poll["short"]);
    st.long_minutes = u32_of(&poll["extended"]);
    st.conveyance_minutes = u32_of(&poll["conveyance"]);
    let status = &data["self_test"]["status"];
    if let Some(code) = num_u64(&status["value"])
        && code >> 4 == 0xF
    {
        st.running = Some(RunningTest {
            kind: None,
            remaining: num_u64(&status["remaining_percent"])
                .map(|p| p.min(100) as u8)
                .or(Some(((code & 0xF) * 10) as u8)),
        });
    }
    let log = &v["ata_smart_self_test_log"];
    let table = log["extended"]["table"].as_array().or_else(|| log["standard"]["table"].as_array());
    if let Some(table) = table {
        st.log = table
            .iter()
            .map(|e| {
                let code = num_u64(&e["status"]["value"]).unwrap_or(0);
                SelfTestEntry {
                    kind: ata_test_kind(num_u64(&e["type"]["value"]), e["type"]["string"].as_str().unwrap_or("")),
                    status: text(&e["status"]["string"]).unwrap_or_default(),
                    outcome: ata_outcome(code),
                    hours: num_u64(&e["lifetime_hours"]),
                    lba: num_u64(&e["lba"]),
                    remaining: num_u64(&e["status"]["remaining_percent"]).map(|p| p.min(100) as u8),
                }
            })
            .collect();
    }
    if let Some(running) = &mut st.running
        && let Some(first) = st.log.first()
        && first.outcome == TestOutcome::Running
    {
        running.kind = Some(first.kind.clone());
    }
}

/// ATA self-test type codes: 1/129 short, 2/130 extended, 3/131 conveyance,
/// 4/132 selective, 0 offline data collection.
fn ata_test_kind(code: Option<u64>, label: &str) -> String {
    let by_code = match code.map(|c| c & 0x7F) {
        Some(0) => Some("offline"),
        Some(1) => Some("short"),
        Some(2) => Some("long"),
        Some(3) => Some("conveyance"),
        Some(4) => Some("selective"),
        _ => None,
    };
    let lower = label.to_ascii_lowercase();
    by_code
        .or_else(|| {
            if lower.contains("short") {
                Some("short")
            } else if lower.contains("extended") || lower.contains("long") {
                Some("long")
            } else if lower.contains("conveyance") {
                Some("conveyance")
            } else if lower.contains("selective") {
                Some("selective")
            } else if lower.contains("offline") {
                Some("offline")
            } else {
                None
            }
        })
        .unwrap_or("vendor")
        .to_string()
}

/// The high nibble of an ATA self-test status byte.
pub fn ata_outcome(code: u64) -> TestOutcome {
    match code >> 4 {
        0 => TestOutcome::Passed,
        1 => TestOutcome::Aborted,
        2 => TestOutcome::Interrupted,
        3..=8 => TestOutcome::Failed,
        15 => TestOutcome::Running,
        _ => TestOutcome::Unknown,
    }
}

fn nvme(r: &mut Report, v: &Value) {
    let log = &v["nvme_smart_health_information_log"];
    if log.is_object() {
        r.critical_warning = u32_of(&log["critical_warning"]);
        r.temperature = r.temperature.or_else(|| num_f64(&log["temperature"]));
        r.available_spare = u32_of(&log["available_spare"]);
        r.spare_threshold = u32_of(&log["available_spare_threshold"]);
        r.wear_used = num_f64(&log["percentage_used"]);
        // One "data unit" is 1000 blocks of 512 bytes.
        r.bytes_read = num_u64(&log["data_units_read"]).map(|n| n.saturating_mul(512_000));
        r.bytes_written = num_u64(&log["data_units_written"]).map(|n| n.saturating_mul(512_000));
        r.power_cycles = r.power_cycles.or_else(|| num_u64(&log["power_cycles"]));
        r.power_on_hours = r.power_on_hours.or_else(|| num_u64(&log["power_on_hours"]));
        r.unsafe_shutdowns = num_u64(&log["unsafe_shutdowns"]);
        r.media_errors = num_u64(&log["media_errors"]);
        r.error_log_entries = num_u64(&log["num_err_log_entries"]);
    }
    let tests = &v["nvme_self_test_log"];
    if tests.is_object() {
        let st = &mut r.self_tests;
        st.short = true;
        st.long = true;
        // The NVMe spec caps a short test at two minutes.
        st.short_minutes = Some(2);
        st.long_minutes = u32_of(&v["nvme_extended_self_test_minutes"]);
        let current = num_u64(&tests["current_self_test_operation"]["value"]).unwrap_or(0);
        if current != 0 {
            let done = num_u64(&tests["current_self_test_completion_percent"]);
            st.running = Some(RunningTest {
                kind: Some(if current == 2 { "long" } else { "short" }.to_string()),
                remaining: done.map(|d| 100u64.saturating_sub(d) as u8),
            });
        }
        if let Some(table) = tests["table"].as_array() {
            st.log = table
                .iter()
                .map(|e| {
                    let code = num_u64(&e["self_test_result"]["value"]).unwrap_or(15);
                    SelfTestEntry {
                        kind: match num_u64(&e["self_test_code"]["value"]) {
                            Some(1) => "short",
                            Some(2) => "long",
                            _ => "vendor",
                        }
                        .to_string(),
                        status: text(&e["self_test_result"]["string"]).unwrap_or_default(),
                        outcome: nvme_outcome(code),
                        hours: num_u64(&e["power_on_hours"]),
                        lba: num_u64(&e["lba"]),
                        remaining: None,
                    }
                })
                .collect();
        }
    }
}

/// NVMe self-test result codes (Device Self-test log, bits 3:0).
pub fn nvme_outcome(code: u64) -> TestOutcome {
    match code {
        0 => TestOutcome::Passed,
        1 | 4 | 9 => TestOutcome::Aborted,
        2 | 3 | 8 => TestOutcome::Interrupted,
        5..=7 => TestOutcome::Failed,
        _ => TestOutcome::Unknown,
    }
}

fn scsi(r: &mut Report, v: &Value) {
    r.grown_defects = num_u64(&v["scsi_grown_defect_list"]);
    r.wear_used = num_f64(&v["scsi_percentage_used_endurance_indicator"]);
    let cycles = &v["scsi_start_stop_cycle_counter"];
    r.power_cycles = r.power_cycles.or_else(|| num_u64(&cycles["accumulated_start_stop_cycles"]));
    r.start_stops = num_u64(&cycles["accumulated_start_stop_cycles"]);
    r.load_cycles = num_u64(&cycles["accumulated_load_unload_cycles"]);
    let log = &v["scsi_error_counter_log"];
    if log.is_object() {
        let total: u64 =
            ["read", "write", "verify"].iter().filter_map(|k| num_u64(&log[k]["total_uncorrected_errors"])).sum();
        r.uncorrected_errors = Some(total);
    }
    let st = &mut r.self_tests;
    st.short = r.smart_available.unwrap_or(true);
    st.long = st.short;
    st.long_minutes = num_u64(&v["scsi_extended_self_test_seconds"]).map(|s| s.div_ceil(60) as u32);
    let mut entries: Vec<(u64, &Value)> = v
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, e)| k.strip_prefix("scsi_self_test_").and_then(|n| n.parse().ok()).map(|n| (n, e)))
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by_key(|(n, _)| *n);
    st.log = entries
        .into_iter()
        .map(|(_, e)| {
            let code = num_u64(&e["result"]["value"]).unwrap_or(15);
            SelfTestEntry {
                kind: match num_u64(&e["code"]["value"]) {
                    Some(1) | Some(5) => "short",
                    Some(2) | Some(6) => "long",
                    _ => "vendor",
                }
                .to_string(),
                status: text(&e["result"]["string"]).unwrap_or_default(),
                outcome: match code {
                    0 => TestOutcome::Passed,
                    1 | 2 => TestOutcome::Aborted,
                    3..=7 => TestOutcome::Failed,
                    15 => TestOutcome::Running,
                    _ => TestOutcome::Unknown,
                },
                hours: num_u64(&e["power_on_time"]["hours"]),
                lba: num_u64(&e["lba_first_failure"]["value"]).or_else(|| num_u64(&e["lba_first_failure"])),
                remaining: None,
            }
        })
        .collect();
    if st.log.first().is_some_and(|e| e.outcome == TestOutcome::Running) {
        st.running = Some(RunningTest { kind: st.log.first().map(|e| e.kind.clone()), remaining: None });
    }
}

/// Fills the headline numbers from the ATA attribute table. IDs are mostly
/// universal; where vendors reuse an ID, the name decides.
fn apply_attributes(r: &mut Report, sector: u64) {
    let attrs = std::mem::take(&mut r.attributes);
    for a in &attrs {
        let raw = first_uint(&a.raw);
        let name = a.name.as_str();
        match a.id {
            4 => r.start_stops = raw,
            5 => r.reallocated = raw,
            9 if r.power_on_hours.is_none() => {
                r.power_on_hours = raw.map(|n| {
                    if name.contains("Half_Minutes") {
                        n / 120
                    } else if name.contains("Minutes") {
                        n / 60
                    } else if name.contains("Seconds") {
                        n / 3600
                    } else {
                        n
                    }
                })
            }
            10 => r.spin_retries = raw,
            12 if r.power_cycles.is_none() => r.power_cycles = raw,
            22 if name.contains("Helium") => r.helium = a.value,
            187 => r.reported_uncorrect = raw,
            188 if name.contains("Timeout") => r.command_timeouts = raw,
            193 | 225 if name.contains("Load_Cycle") => r.load_cycles = raw,
            196 => r.reallocated_events = raw,
            197 => r.pending = raw,
            198 => r.offline_uncorrectable = raw,
            199 => r.crc_errors = raw,
            241 if r.bytes_written.is_none() => r.bytes_written = lba_counter(name, raw, sector),
            242 if r.bytes_read.is_none() => r.bytes_read = lba_counter(name, raw, sector),
            _ => {}
        }
        if r.wear_used.is_none() && is_wear_attribute(name) {
            r.wear_used = a.value.map(|v| f64::from(100u16.saturating_sub(v.min(100))));
        }
    }
    r.attributes = attrs;
}

/// Attributes whose normalized value is the remaining life in percent.
fn is_wear_attribute(name: &str) -> bool {
    ["Wear_Leveling", "Wearout", "Life_Left", "Lifetime_Remain", "Life_Remain", "Remaining_Lifetime"]
        .iter()
        .any(|k| name.contains(k))
}

/// Total LBAs written/read, in whatever unit the vendor counts.
fn lba_counter(name: &str, raw: Option<u64>, sector: u64) -> Option<u64> {
    let n = raw?;
    let unit = if name.contains("32MiB") {
        32 << 20
    } else if name.contains("GiB") || name.contains("_GB") {
        1 << 30
    } else if name.contains("MiB") {
        1 << 20
    } else if name.contains("LBA") || name.contains("Sector") {
        sector
    } else {
        return None;
    };
    n.checked_mul(unit)
}

fn attribute_temperature(attrs: &[Attribute]) -> Option<f64> {
    let pick =
        |id: u16| attrs.iter().find(|a| a.id == id).and_then(|a| first_uint(&a.raw)).filter(|t| (1..=120).contains(t));
    pick(194)
        .or_else(|| pick(190))
        .or_else(|| {
            attrs
                .iter()
                .find(|a| a.name.contains("Temperature"))
                .and_then(|a| first_uint(&a.raw))
                .filter(|t| (1..=120).contains(t))
        })
        .map(|t| t as f64)
}

/// The ATA device statistics log (GP log 0x04) has exact counters some
/// drives no longer keep as attributes.
fn apply_device_statistics(r: &mut Report, stats: &Value, sector: u64) {
    let Some(pages) = stats["pages"].as_array() else { return };
    for entry in pages.iter().filter_map(|p| p["table"].as_array()).flatten() {
        let Some(name) = entry["name"].as_str() else { continue };
        let valid = entry["flags"]["valid"].as_bool().unwrap_or(true);
        let Some(value) = num_u64(&entry["value"]).filter(|_| valid) else { continue };
        match name {
            "Logical Sectors Written" => r.bytes_written = Some(value.saturating_mul(sector)),
            "Logical Sectors Read" => r.bytes_read = Some(value.saturating_mul(sector)),
            "Percentage Used Endurance Indicator" => r.wear_used = Some(value as f64),
            "Highest Temperature" if r.temp_max_lifetime.is_none() => r.temp_max_lifetime = Some(value as f64),
            "Lowest Temperature" if r.temp_min_lifetime.is_none() => r.temp_min_lifetime = Some(value as f64),
            "Number of Interface CRC Errors" if r.crc_errors.is_none() => r.crc_errors = Some(value),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The Proxmox API (`GET /nodes/{node}/disks/smart`)
// ---------------------------------------------------------------------------

/// Builds a report from the Proxmox API's SMART data: parsed attributes for
/// ATA disks, a text dump for NVMe and SAS.
pub fn from_pve(v: &Value, now: i64) -> Report {
    let mut r = Report { source: Source::Api, collected_at: now, ..Report::default() };
    let health = v["health"].as_str().unwrap_or("").trim().to_ascii_uppercase();
    r.passed = match health.as_str() {
        "PASSED" | "OK" => Some(true),
        "FAILED" => Some(false),
        _ if health.starts_with("FAILED") => Some(false),
        _ => None,
    };
    if health == "SMART DISABLED" {
        r.smart_available = Some(true);
        r.smart_enabled = Some(false);
    }
    if let Some(attrs) = v["attributes"].as_array() {
        r.protocol = Protocol::Ata;
        r.attributes = attrs
            .iter()
            .filter_map(|a| {
                let id = u16::try_from(num_u64(&a["id"])?).ok()?;
                let flags = a["flags"].as_str().unwrap_or("");
                Some(Attribute {
                    id,
                    name: text(&a["name"]).unwrap_or_else(|| format!("Attribute_{id}")),
                    value: num_u64(&a["normalized"])
                        .or_else(|| num_u64(&a["value"]))
                        .and_then(|n| u16::try_from(n).ok()),
                    worst: num_u64(&a["worst"]).and_then(|n| u16::try_from(n).ok()),
                    thresh: num_u64(&a["threshold"]).and_then(|n| u16::try_from(n).ok()),
                    raw: text(&a["raw"]).unwrap_or_default(),
                    // "Brief" flags: P marks a pre-failure attribute.
                    prefail: flags.starts_with('P'),
                    failed: match a["fail"].as_str().unwrap_or("-") {
                        "FAILING_NOW" => "now".into(),
                        "In_the_past" => "past".into(),
                        _ => String::new(),
                    },
                })
            })
            .collect();
        apply_attributes(&mut r, 512);
        r.temperature = attribute_temperature(&r.attributes);
    } else if let Some(t) = v["text"].as_str() {
        r.text = Some(t.to_string());
        parse_text_dump(&mut r, t);
    }
    if r.wear_used.is_none() {
        // Proxmox's own wearout is the remaining life in percent.
        r.wear_used = num_f64(&v["wearout"]).filter(|w| (0.0..=100.0).contains(w)).map(|w| 100.0 - w);
    }
    if r.attributes.is_empty() && r.text.is_none() && r.passed.is_some() {
        r.smart_available = Some(true);
    }
    r
}

/// Reads the NVMe and SAS fields of a smartctl text dump.
fn parse_text_dump(r: &mut Report, t: &str) {
    for line in t.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Critical Warning" => {
                r.protocol = Protocol::Nvme;
                r.critical_warning = value
                    .strip_prefix("0x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| first_uint(value).map(|n| n as u32));
            }
            "Temperature" | "Current Drive Temperature" => {
                if key.starts_with("Current") {
                    r.protocol = Protocol::Scsi;
                }
                r.temperature = first_uint(value).map(|n| n as f64);
            }
            "Drive Trip Temperature" => r.temp_critical = first_uint(value).map(|n| n as f64),
            "Warning  Comp. Temp. Threshold" | "Warning Comp. Temp. Threshold" => {
                r.temp_warning = first_uint(value).map(|n| n as f64)
            }
            "Critical Comp. Temp. Threshold" => r.temp_critical = first_uint(value).map(|n| n as f64),
            "Available Spare" => r.available_spare = first_uint(value).map(|n| n as u32),
            "Available Spare Threshold" => r.spare_threshold = first_uint(value).map(|n| n as u32),
            "Percentage Used" | "Percentage used endurance indicator" => {
                r.wear_used = first_uint(value).map(|n| n as f64)
            }
            "Data Units Read" => r.bytes_read = first_uint(value).map(|n| n.saturating_mul(512_000)),
            "Data Units Written" => r.bytes_written = first_uint(value).map(|n| n.saturating_mul(512_000)),
            "Power Cycles" => r.power_cycles = first_uint(value),
            "Accumulated start-stop cycles" => {
                r.power_cycles = first_uint(value);
                r.start_stops = first_uint(value);
            }
            "Accumulated load-unload cycles" => r.load_cycles = first_uint(value),
            "Power On Hours" => r.power_on_hours = first_uint(value),
            "Accumulated power on time, hours" | "Accumulated power on time, hours:minutes" => {
                r.power_on_hours = first_uint(value)
            }
            "Unsafe Shutdowns" => r.unsafe_shutdowns = first_uint(value),
            "Media and Data Integrity Errors" => r.media_errors = first_uint(value),
            "Error Information Log Entries" => r.error_log_entries = first_uint(value),
            "Elements in grown defect list" => {
                r.protocol = Protocol::Scsi;
                r.grown_defects = first_uint(value)
            }
            _ => {}
        }
    }
    if r.protocol == Protocol::Unknown && (r.available_spare.is_some() || r.media_errors.is_some()) {
        r.protocol = Protocol::Nvme;
    }
}

// ---------------------------------------------------------------------------
// Assessment
// ---------------------------------------------------------------------------

/// How much a finding matters. `Info` never changes a disk's verdict.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    Ok,
    Info,
    Watch,
    Warning,
    Failing,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Info => "info",
            Level::Watch => "watch",
            Level::Warning => "warning",
            Level::Failing => "failing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub level: Level,
    /// Stable identifier, e.g. "pending" or "temperature".
    pub code: String,
    pub title: String,
    pub detail: String,
}

impl Finding {
    fn new(level: Level, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { level, code: code.into(), title: title.into(), detail: detail.into() }
    }
}

/// How counters changed recently (from Nodestarr's own history).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Trends {
    /// Days the deltas cover (0 when there is no older sample).
    pub days: f64,
    pub reallocated: Option<i64>,
    pub pending: Option<i64>,
    pub uncorrectable: Option<i64>,
    pub crc: Option<i64>,
    pub media_errors: Option<i64>,
    pub load_cycles: Option<i64>,
}

/// Whether a disk spins down between readings, learned from its start count.
/// Reading SMART data through the Proxmox API wakes a sleeping disk, so such
/// disks are read less often.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpinDown {
    /// Readings in a row that found the spindle had started since the one before.
    pub streak: u8,
    pub sleeps: bool,
}

impl SpinDown {
    /// Readings in a row with a new start before a disk counts as sleeping in
    /// between: one alone can be a coincidence (a reboot, a backup waking it).
    const LEARN: u8 = 2;

    /// The state after a reading, from the start count at the reading before
    /// and at this one.
    pub fn next(self, before: Option<u64>, now: Option<u64>) -> Self {
        match (before, now) {
            (Some(b), Some(n)) if n > b => {
                let streak = self.streak.saturating_add(1);
                Self { streak, sleeps: self.sleeps || streak >= Self::LEARN }
            }
            // It stayed awake since the last reading.
            (Some(b), Some(n)) if n == b => Self::default(),
            _ => self,
        }
    }
}

/// What else is known about the disk when judging its report.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub media: Media,
    /// Temperature limit for hard disks; flash uses the drive's own limit or
    /// this plus 20 °C.
    pub hdd_temp_limit: f64,
    pub trends: Trends,
    /// Member of a ZFS pool, md array or SnapRAID array.
    pub in_zfs: bool,
    pub in_md: bool,
    /// The newest finished self-test (Nodestarr's or the drive's own log).
    pub last_test: Option<SelfTestEntry>,
    /// Self-tests can be run (host helper available and the drive supports them).
    pub can_test: bool,
}

/// The temperature limit that applies to a disk.
pub fn temp_limit(r: &Report, ctx: &Context) -> f64 {
    if ctx.media.is_flash() {
        r.temp_warning.filter(|t| (40.0..=100.0).contains(t)).unwrap_or(ctx.hdd_temp_limit + 20.0)
    } else {
        ctx.hdd_temp_limit
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn increased(delta: Option<i64>) -> Option<u64> {
    delta.filter(|d| *d > 0).map(|d| d as u64)
}

/// Judges a report. Findings come back most serious first.
pub fn assess(r: &Report, ctx: &Context) -> Vec<Finding> {
    let mut out = Vec::new();
    let days = if ctx.trends.days >= 1.0 { format!("{:.0} days", ctx.trends.days) } else { "recently".into() };

    if r.smart_available == Some(false) {
        out.push(Finding::new(
            Level::Info,
            "no_smart",
            "No SMART data",
            "This disk doesn't report SMART health. That's common behind USB bridges and hardware RAID controllers.",
        ));
    } else if r.smart_enabled == Some(false) {
        out.push(Finding::new(
            Level::Watch,
            "smart_disabled",
            "SMART is turned off",
            "The disk supports SMART but has it disabled, so it can't warn about problems. Turn it on with `smartctl -s on`.",
        ));
    }
    if r.passed == Some(false) {
        out.push(Finding::new(
            Level::Failing,
            "smart_failed",
            "The disk reports it is failing",
            "Its own SMART health check failed. Copy off anything important and replace the disk.",
        ));
    }

    for a in &r.attributes {
        let name = a.name.replace('_', " ");
        if a.failed == "now" {
            out.push(Finding::new(
                if a.prefail { Level::Failing } else { Level::Warning },
                &format!("attr_{}", a.id),
                format!("{name} is past its failure threshold"),
                format!(
                    "The normalized value {} is at or below the vendor's threshold {}. {}",
                    a.value.map(|v| v.to_string()).unwrap_or_default(),
                    a.thresh.map(|v| v.to_string()).unwrap_or_default(),
                    if a.prefail {
                        "The vendor treats this as a sign of imminent failure."
                    } else {
                        "This usually reflects age or use rather than imminent failure."
                    }
                ),
            ));
        } else if a.failed == "past" {
            out.push(Finding::new(
                Level::Watch,
                &format!("attr_{}", a.id),
                format!("{name} crossed its threshold in the past"),
                "It has recovered since, but the drive keeps a record that it happened.".to_string(),
            ));
        }
    }

    if let Some(n) = r.pending.filter(|n| *n > 0) {
        out.push(Finding::new(
            Level::Warning,
            "pending",
            format!("{} waiting to be reallocated", plural(n, "unreadable sector", "unreadable sectors")),
            "The disk couldn't read these sectors. Whatever was stored there is unreadable until it's written again, \
             when the disk swaps in spare sectors. Run a long self-test (and a scrub, if the disk is in a pool) to \
             find out whether the damage is spreading.",
        ));
    }
    if let Some(n) = r.offline_uncorrectable.filter(|n| *n > 0) {
        out.push(Finding::new(
            Level::Warning,
            "uncorrectable",
            format!("{} the disk couldn't read during its own scans", plural(n, "sector", "sectors")),
            "These failed in the disk's offline surface scan. It usually goes together with pending sectors.",
        ));
    }
    if let Some(n) = r.reallocated.filter(|n| *n > 0) {
        let grew = increased(ctx.trends.reallocated);
        let (level, detail) = match grew {
            Some(d) => (
                Level::Warning,
                format!(
                    "{} more in the last {days}. A growing number of reallocated sectors usually means the surface \
                     is deteriorating; plan to replace the disk.",
                    d
                ),
            ),
            None if n > 100 => (
                Level::Warning,
                "That's a lot of replaced sectors, even though the number isn't growing right now.".to_string(),
            ),
            None => (
                Level::Watch,
                "The disk replaced bad sectors with spares. A small, stable number is not alarming; it matters if it \
                 keeps growing."
                    .to_string(),
            ),
        };
        out.push(Finding::new(level, "reallocated", format!("{} reallocated", plural(n, "sector", "sectors")), detail));
    }
    if let Some(n) = r.reported_uncorrect.filter(|n| *n > 0) {
        out.push(Finding::new(
            Level::Watch,
            "reported_uncorrect",
            format!("{} the disk couldn't correct", plural(n, "read error", "read errors")),
            "Reads failed even after error correction. If the number grows, expect pending or reallocated sectors next.",
        ));
    }
    if let Some(n) = r.crc_errors.filter(|n| *n > 0) {
        match increased(ctx.trends.crc) {
            Some(d) => out.push(Finding::new(
                Level::Watch,
                "crc",
                format!("{} new cable errors", d),
                format!(
                    "The link between the disk and the controller corrupted data {} in the last {days}. This is \
                     almost always the SATA cable or connector, not the disk: reseat or replace the cable.",
                    plural(d, "time", "times")
                ),
            )),
            None => out.push(Finding::new(
                Level::Info,
                "crc",
                format!("{} cable errors in the past", n),
                "Interface CRC errors never reset. None were added recently, so whatever caused them (often a loose \
                 cable) seems fixed.",
            )),
        }
    }
    if let Some(n) = r.spin_retries.filter(|n| *n > 0) {
        out.push(Finding::new(
            Level::Watch,
            "spin_retries",
            format!("Needed {} to spin up", plural(n, "retry", "retries")),
            "The motor had trouble reaching speed. Check the power supply and cabling; it can also be the motor itself.",
        ));
    }
    if let Some(n) = r.grown_defects.filter(|n| *n > 0) {
        out.push(Finding::new(
            if increased(ctx.trends.reallocated).is_some() { Level::Warning } else { Level::Watch },
            "grown_defects",
            format!("{} in the grown defect list", plural(n, "sector", "sectors")),
            "The disk has remapped sectors since it left the factory. Watch whether the number grows.",
        ));
    }
    if let Some(n) = r.uncorrected_errors.filter(|n| *n > 0) {
        out.push(Finding::new(
            Level::Warning,
            "uncorrected",
            format!("{} uncorrected errors", n),
            "Reads, writes or verifies failed even after the drive's error correction.",
        ));
    }

    // Flash health.
    if let Some(cw) = r.critical_warning.filter(|c| *c != 0) {
        let mut parts = Vec::new();
        let mut level = Level::Warning;
        if cw & 0x01 != 0 {
            parts.push("spare capacity is below its threshold");
            level = Level::Failing;
        }
        if cw & 0x02 != 0 {
            parts.push("temperature is outside its limits");
        }
        if cw & 0x04 != 0 {
            parts.push("reliability is degraded by media errors");
            level = Level::Failing;
        }
        if cw & 0x08 != 0 {
            parts.push("it has switched to read-only");
            level = Level::Failing;
        }
        if cw & 0x10 != 0 {
            parts.push("its volatile memory backup failed");
        }
        if cw & 0x20 != 0 {
            parts.push("its persistent memory region is read-only");
        }
        out.push(Finding::new(
            level,
            "critical_warning",
            "The drive raised a critical warning",
            format!(
                "It reports that {}.",
                if parts.is_empty() { "something is wrong".into() } else { parts.join(", ") }
            ),
        ));
    }
    if let (Some(spare), Some(thresh)) = (r.available_spare, r.spare_threshold)
        && thresh > 0
    {
        if spare < thresh {
            out.push(Finding::new(
                Level::Failing,
                "spare",
                format!("Spare capacity down to {spare}%"),
                format!("Below the drive's own minimum of {thresh}%. It may turn read-only; replace it."),
            ));
        } else if spare <= thresh + 10 && spare < 100 {
            out.push(Finding::new(
                Level::Watch,
                "spare",
                format!("Spare capacity down to {spare}%"),
                format!("The drive turns unreliable below {thresh}%."),
            ));
        }
    }
    if let Some(n) = r.media_errors.filter(|n| *n > 0) {
        let grew = increased(ctx.trends.media_errors);
        out.push(Finding::new(
            if grew.is_some() { Level::Warning } else { Level::Watch },
            "media_errors",
            format!("{} media errors", n),
            match grew {
                Some(d) => format!("{d} new in the last {days}: the flash is returning data it can't correct."),
                None => "The drive returned data it couldn't correct at some point. None were added recently.".into(),
            },
        ));
    }
    if let Some(w) = r.wear_used {
        if w >= 100.0 {
            out.push(Finding::new(
                Level::Warning,
                "wear",
                "Rated endurance used up",
                format!(
                    "{w:.0}% of the writes the vendor rates it for. It can keep working for a long time, but it's \
                     outside the warranty's endurance; watch for errors and keep backups."
                ),
            ));
        } else if w >= 90.0 {
            out.push(Finding::new(
                Level::Warning,
                "wear",
                format!("{w:.0}% of its rated endurance used"),
                "Plan a replacement before it wears out.",
            ));
        } else if w >= 75.0 {
            out.push(Finding::new(
                Level::Watch,
                "wear",
                format!("{w:.0}% of its rated endurance used"),
                "Fine for now; it's past three quarters of the writes it's rated for.",
            ));
        }
    }

    // Temperature.
    if let Some(t) = r.temperature {
        let limit = temp_limit(r, ctx);
        if let Some(crit) = r.temp_critical.filter(|c| t >= *c && *c > 0.0) {
            out.push(Finding::new(
                Level::Failing,
                "temperature",
                format!("Overheating at {t:.0} °C"),
                format!("Above the drive's absolute limit of {crit:.0} °C. Check cooling right away."),
            ));
        } else if t > limit {
            out.push(Finding::new(
                Level::Warning,
                "temperature",
                format!("Running hot at {t:.0} °C"),
                format!("Above the {limit:.0} °C limit. Heat shortens a disk's life; check airflow and fans."),
            ));
        } else if let Some(max) = r.temp_max_lifetime.filter(|m| *m > limit + 5.0) {
            out.push(Finding::new(
                Level::Info,
                "temperature_history",
                format!("Has been as hot as {max:.0} °C"),
                "The drive recorded that at some point in its life; it's fine now.",
            ));
        }
    }
    if let Some(h) = r.helium.filter(|h| *h < 100) {
        out.push(Finding::new(
            if h <= 25 { Level::Failing } else { Level::Watch },
            "helium",
            format!("Helium level at {h}"),
            "Helium-filled drives start at 100. A falling level means the seal leaks; the drive fails when it gets too low.",
        ));
    }
    if let Some(n) = r.load_cycles {
        let per_day =
            increased(ctx.trends.load_cycles).filter(|_| ctx.trends.days >= 1.0).map(|d| d as f64 / ctx.trends.days);
        if n >= 300_000 {
            out.push(Finding::new(
                Level::Watch,
                "load_cycles",
                format!("Heads parked {} times", group_digits(n)),
                "Many drives are rated for 300,000 to 600,000 load cycles.",
            ));
        }
        if let Some(rate) = per_day.filter(|r| *r >= 500.0) {
            out.push(Finding::new(
                Level::Info,
                "load_cycle_rate",
                format!("Parks its heads about {} times a day", group_digits(rate.round() as u64)),
                "An aggressive idle timer (common on WD Green and some Red drives). `idle3ctl` (WD) or `hdparm -B` can \
                 relax it.",
            ));
        }
    }

    // Self-tests.
    if let Some(t) = &ctx.last_test {
        match t.outcome {
            TestOutcome::Failed => out.push(Finding::new(
                Level::Failing,
                "test_failed",
                format!("The last {} self-test failed", t.kind),
                format!(
                    "{}{}. The disk found damage it couldn't work around; back up and replace it.",
                    if t.status.is_empty() { "Failed" } else { &t.status },
                    t.lba.map(|l| format!(" at block {l}")).unwrap_or_default()
                ),
            )),
            TestOutcome::Interrupted | TestOutcome::Aborted => out.push(Finding::new(
                Level::Info,
                "test_interrupted",
                format!("The last {} self-test didn't finish", t.kind),
                format!(
                    "{}. Self-tests stop when the disk is reset, powered off or put to sleep; run it again.",
                    if t.status.is_empty() { "It stopped early" } else { &t.status }
                ),
            )),
            _ => {}
        }
    } else if ctx.can_test && r.self_tests.short && r.self_tests.log.is_empty() {
        out.push(Finding::new(
            Level::Info,
            "never_tested",
            "Never self-tested",
            "The drive's self-test log is empty. A short test takes a couple of minutes and doesn't interrupt anything.",
        ));
    }

    // Context hints.
    if let (Some(max), Some(now)) = (&r.link_max, &r.link_now)
        && link_rate(now) < link_rate(max)
    {
        out.push(Finding::new(
            Level::Info,
            "link_speed",
            format!("Connected at {now}"),
            format!(
                "The disk supports {max}. The port, cable or controller is holding it back (fine for most hard disks, \
                 noticeable for SSDs)."
            ),
        ));
    }
    if ctx.media == Media::Hdd && (ctx.in_zfs || ctx.in_md) && r.model.as_deref().is_some_and(known_smr) {
        out.push(Finding::new(
            Level::Info,
            "smr",
            "SMR disk in a redundant array",
            "This model uses shingled recording (SMR). It works, but rebuilding a RAID-Z vdev or array onto it can take \
             days instead of hours. Prefer CMR disks for replacements.",
        ));
    }
    if ctx.media == Media::Hdd && ctx.in_md && r.erc_read == Some(0) {
        out.push(Finding::new(
            Level::Info,
            "erc",
            "Error recovery control is off",
            "On a bad sector the disk may retry for minutes, and mdadm can drop it from the array meanwhile. \
             `smartctl -l scterc,70,70` limits retries to 7 seconds (until the next power cycle).",
        ));
    }
    if let Some(h) = r.power_on_hours.filter(|h| *h >= 5 * 8766) {
        out.push(Finding::new(
            Level::Info,
            "age",
            format!("{:.1} years of power-on time", h as f64 / 8766.0),
            "Failure rates climb as disks age. Nothing is wrong, but it's a candidate for replacement first.",
        ));
    }

    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    out
}

/// The overall verdict: the most serious finding, with `Info` counting as OK.
pub fn verdict(findings: &[Finding]) -> Level {
    findings.iter().map(|f| f.level).filter(|l| *l > Level::Info).max().unwrap_or(Level::Ok)
}

fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "6.0 Gb/s" → 6.0 (0 when unreadable).
fn link_rate(s: &str) -> f64 {
    s.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0.0)
}

/// Drive-managed SMR models (as disclosed by the vendors in 2020). SMR disks
/// work for most things but rebuild very slowly in RAID-Z vdevs and arrays.
pub fn known_smr(model: &str) -> bool {
    const MODELS: &[&str] = &[
        // WD Red 2-6 TB (EFAX; the 8 TB and larger EFAX are CMR), WD Blue.
        "WD20EFAX",
        "WD30EFAX",
        "WD40EFAX",
        "WD60EFAX",
        "WD20EZAZ",
        "WD60EZAZ",
        "WD10SPZX",
        "WD20SPZX",
        // Seagate BarraCuda and Archive.
        "ST2000DM008",
        "ST3000DM007",
        "ST4000DM004",
        "ST6000DM003",
        "ST8000DM004",
        "ST8000AS0002",
        "ST1000LM048",
        "ST2000LM015",
        "ST3000LM024",
        "ST4000LM024",
        "ST5000LM000",
        // Toshiba P300 and DT02.
        "HDWD240",
        "HDWD260",
        "DT02ABA400",
        "DT02ABA600",
    ];
    let m = model.to_ascii_uppercase();
    MODELS.iter().any(|x| m.contains(x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wd_red() -> Value {
        json!({
            "smartctl": {"version": [7, 3], "exit_status": 0},
            "device": {"name": "/dev/sda", "info_name": "/dev/sda [SAT]", "type": "sat", "protocol": "ATA"},
            "model_family": "Western Digital Red",
            "model_name": "WDC WD40EFRX-68N32N0",
            "serial_number": "WD-WCC7K1234567",
            "firmware_version": "82.00A82",
            "user_capacity": {"blocks": 7814037168u64, "bytes": 4000787030016u64},
            "logical_block_size": 512,
            "physical_block_size": 4096,
            "rotation_rate": 5400,
            "form_factor": {"ata_value": 2, "name": "3.5 inches"},
            "interface_speed": {"max": {"string": "6.0 Gb/s"}, "current": {"string": "3.0 Gb/s"}},
            "smart_support": {"available": true, "enabled": true},
            "smart_status": {"passed": true},
            "ata_smart_data": {
                "self_test": {
                    "status": {"value": 249, "string": "in progress, 90% remaining", "remaining_percent": 90},
                    "polling_minutes": {"short": 2, "extended": 496, "conveyance": 5}
                },
                "capabilities": {"self_tests_supported": true, "conveyance_self_test_supported": true}
            },
            "ata_sct_erc": {"read": {"enabled": false}, "write": {"enabled": true, "deciseconds": 70}},
            "ata_smart_attributes": {"revision": 16, "table": [
                {"id": 1, "name": "Raw_Read_Error_Rate", "value": 200, "worst": 200, "thresh": 51, "when_failed": "",
                 "flags": {"prefailure": true}, "raw": {"value": 0, "string": "0"}},
                {"id": 5, "name": "Reallocated_Sector_Ct", "value": 200, "worst": 200, "thresh": 140, "when_failed": "",
                 "flags": {"prefailure": true}, "raw": {"value": 8, "string": "8"}},
                {"id": 9, "name": "Power_On_Hours", "value": 44, "worst": 44, "thresh": 0, "when_failed": "",
                 "flags": {"prefailure": false}, "raw": {"value": 41234, "string": "41234"}},
                {"id": 193, "name": "Load_Cycle_Count", "value": 180, "worst": 180, "thresh": 0, "when_failed": "",
                 "flags": {"prefailure": false}, "raw": {"value": 61234, "string": "61234"}},
                {"id": 194, "name": "Temperature_Celsius", "value": 116, "worst": 104, "thresh": 0, "when_failed": "",
                 "flags": {"prefailure": false}, "raw": {"value": 197568495650u64, "string": "34 (Min/Max 20/46)"}},
                {"id": 197, "name": "Current_Pending_Sector", "value": 200, "worst": 200, "thresh": 0, "when_failed": "",
                 "flags": {"prefailure": false}, "raw": {"value": 2, "string": "2"}},
                {"id": 199, "name": "UDMA_CRC_Error_Count", "value": 200, "worst": 200, "thresh": 0, "when_failed": "",
                 "flags": {"prefailure": false}, "raw": {"value": 3, "string": "3"}}
            ]},
            "power_on_time": {"hours": 41234},
            "power_cycle_count": 54,
            "temperature": {"current": 34, "lifetime_min": 15, "lifetime_max": 49, "op_limit_max": 60, "limit_max": 70},
            "ata_smart_error_log": {"extended": {"revision": 1, "sectors": 2, "count": 0}},
            "ata_smart_self_test_log": {"extended": {"revision": 1, "sectors": 1, "table": [
                {"type": {"value": 1, "string": "Short offline"}, "status": {"value": 249, "string": "Self-test routine in progress", "remaining_percent": 90}, "lifetime_hours": 41234},
                {"type": {"value": 2, "string": "Extended offline"}, "status": {"value": 121, "string": "Completed: read failure", "remaining_percent": 90, "passed": false}, "lifetime_hours": 41100, "lba": 123456789},
                {"type": {"value": 1, "string": "Short offline"}, "status": {"value": 0, "string": "Completed without error", "passed": true}, "lifetime_hours": 41000}
            ]}},
            "ata_device_statistics": {"pages": [{"number": 1, "name": "General Statistics", "table": [
                {"offset": 24, "name": "Logical Sectors Written", "size": 6, "value": 1000, "flags": {"valid": true}},
                {"offset": 40, "name": "Logical Sectors Read", "size": 6, "value": 2000, "flags": {"valid": true}}
            ]}]}
        })
    }

    #[test]
    fn parses_ata_json() {
        let r = from_smartctl(&wd_red(), 100);
        assert_eq!(r.protocol, Protocol::Ata);
        assert_eq!(r.source, Source::Helper);
        assert_eq!(r.model.as_deref(), Some("WDC WD40EFRX-68N32N0"));
        assert_eq!(r.serial.as_deref(), Some("WD-WCC7K1234567"));
        assert_eq!(r.capacity, Some(4000787030016));
        assert_eq!(r.rpm, Some(5400));
        assert_eq!(r.passed, Some(true));
        assert_eq!(r.temperature, Some(34.0));
        assert_eq!(r.temp_max_lifetime, Some(49.0));
        assert_eq!(r.temp_warning, Some(60.0));
        assert_eq!(r.temp_critical, Some(70.0));
        assert_eq!(r.power_on_hours, Some(41234));
        assert_eq!(r.power_cycles, Some(54));
        assert_eq!(r.reallocated, Some(8));
        assert_eq!(r.pending, Some(2));
        assert_eq!(r.crc_errors, Some(3));
        assert_eq!(r.load_cycles, Some(61234));
        assert_eq!(r.erc_read, Some(0));
        assert_eq!(r.erc_write, Some(70));
        assert_eq!(r.bytes_written, Some(512_000));
        assert_eq!(r.bytes_read, Some(1_024_000));
        assert_eq!(r.link_now.as_deref(), Some("3.0 Gb/s"));
        assert_eq!(r.error_log_entries, Some(0));
        let st = &r.self_tests;
        assert!(st.short && st.long && st.conveyance);
        assert_eq!(st.long_minutes, Some(496));
        assert_eq!(st.running, Some(RunningTest { kind: Some("short".into()), remaining: Some(90) }));
        assert_eq!(st.log.len(), 3);
        assert_eq!(st.log[1].kind, "long");
        assert_eq!(st.log[1].outcome, TestOutcome::Failed);
        assert_eq!(st.log[1].lba, Some(123456789));
        assert_eq!(st.log[2].outcome, TestOutcome::Passed);
        assert!(r.attributes.iter().any(|a| a.id == 5 && a.prefail));
    }

    #[test]
    fn parses_nvme_json() {
        let v = json!({
            "device": {"name": "/dev/nvme0", "type": "nvme", "protocol": "NVMe"},
            "model_name": "Samsung SSD 970 EVO Plus 1TB",
            "serial_number": "S4EWNX0R123456",
            "nvme_total_capacity": 1000204886016u64,
            "smart_support": {"available": true, "enabled": true},
            "smart_status": {"passed": true, "nvme": {"value": 0}},
            "nvme_smart_health_information_log": {
                "critical_warning": 0, "temperature": 38, "available_spare": 100, "available_spare_threshold": 10,
                "percentage_used": 2, "data_units_read": 1000, "data_units_written": 2000, "power_cycles": 123,
                "power_on_hours": 12345, "unsafe_shutdowns": 45, "media_errors": 0, "num_err_log_entries": 7
            },
            "temperature": {"current": 38, "op_limit_max": 85, "critical_limit_max": 85},
            "power_on_time": {"hours": 12345},
            "nvme_self_test_log": {
                "current_self_test_operation": {"value": 1, "string": "Short self-test in progress"},
                "current_self_test_completion_percent": 30,
                "table": [{"self_test_code": {"value": 2, "string": "Extended"}, "self_test_result": {"value": 0, "string": "Completed without error"}, "power_on_hours": 12000}]
            }
        });
        let r = from_smartctl(&v, 0);
        assert_eq!(r.protocol, Protocol::Nvme);
        assert_eq!(r.capacity, Some(1000204886016));
        assert_eq!(r.wear_used, Some(2.0));
        assert_eq!(r.bytes_written, Some(1_024_000_000));
        assert_eq!(r.available_spare, Some(100));
        assert_eq!(r.temp_warning, Some(85.0));
        assert_eq!(r.self_tests.running.as_ref().and_then(|t| t.remaining), Some(70));
        assert_eq!(r.self_tests.log[0].kind, "long");
        assert_eq!(r.self_tests.log[0].outcome, TestOutcome::Passed);
        assert!(r.self_tests.short && r.self_tests.long && !r.self_tests.conveyance);
    }

    #[test]
    fn parses_scsi_json() {
        let v = json!({
            "device": {"name": "/dev/sdb", "type": "scsi", "protocol": "SCSI"},
            "scsi_vendor": "SEAGATE", "scsi_product": "ST4000NM0023", "scsi_model_name": "SEAGATE ST4000NM0023",
            "serial_number": "Z1Z0ABCD",
            "smart_support": {"available": true, "enabled": true},
            "smart_status": {"passed": true},
            "temperature": {"current": 30, "drive_trip": 68},
            "scsi_grown_defect_list": 4,
            "scsi_start_stop_cycle_counter": {"accumulated_start_stop_cycles": 120, "accumulated_load_unload_cycles": 1500},
            "scsi_error_counter_log": {"read": {"total_uncorrected_errors": 0}, "write": {"total_uncorrected_errors": 1}},
            "power_on_time": {"hours": 30000, "minutes": 12},
            "scsi_self_test_0": {"code": {"value": 1, "string": "Background short"}, "result": {"value": 0, "string": "Completed"}, "power_on_time": {"hours": 29990}}
        });
        let r = from_smartctl(&v, 0);
        assert_eq!(r.protocol, Protocol::Scsi);
        assert_eq!(r.model.as_deref(), Some("SEAGATE ST4000NM0023"));
        assert_eq!(r.temp_critical, Some(68.0));
        assert_eq!(r.grown_defects, Some(4));
        assert_eq!(r.uncorrected_errors, Some(1));
        assert_eq!(r.power_cycles, Some(120));
        assert_eq!(r.self_tests.log[0].outcome, TestOutcome::Passed);
        assert_eq!(r.self_tests.log[0].kind, "short");
    }

    #[test]
    fn parses_proxmox_attributes() {
        let v = json!({
            "health": "PASSED",
            "type": "ata",
            "attributes": [
                {"id": "  5", "name": "Reallocated_Sector_Ct", "value": 100, "normalized": 100, "worst": 100, "threshold": 10, "fail": "-", "flags": "PO--CK", "raw": "0"},
                {"id": "  9", "name": "Power_On_Minutes", "value": 99, "normalized": 99, "worst": 99, "threshold": 0, "fail": "-", "flags": "-O--CK", "raw": "120"},
                {"id": "177", "name": "Wear_Leveling_Count", "value": 97, "normalized": 97, "worst": 97, "threshold": 0, "fail": "-", "flags": "PO--C-", "raw": "31"},
                {"id": "190", "name": "Airflow_Temperature_Cel", "value": 64, "normalized": 64, "worst": 50, "threshold": 0, "fail": "In_the_past", "flags": "-O--CK", "raw": "36"},
                {"id": "241", "name": "Total_LBAs_Written", "value": 99, "normalized": 99, "worst": 99, "threshold": 0, "fail": "-", "flags": "-O--CK", "raw": "1000"}
            ]
        });
        let r = from_pve(&v, 5);
        assert_eq!(r.source, Source::Api);
        assert_eq!(r.protocol, Protocol::Ata);
        assert_eq!(r.passed, Some(true));
        assert_eq!(r.reallocated, Some(0));
        assert_eq!(r.power_on_hours, Some(2));
        assert_eq!(r.wear_used, Some(3.0));
        assert_eq!(r.temperature, Some(36.0));
        assert_eq!(r.bytes_written, Some(512_000));
        assert!(r.attributes[0].prefail);
        assert_eq!(r.attributes[3].failed, "past");
    }

    #[test]
    fn parses_proxmox_nvme_text() {
        let text = "SMART/Health Information (NVMe Log 0x02)\nCritical Warning:                   0x04\n\
                    Temperature:                        41 Celsius\nAvailable Spare:                    9%\n\
                    Available Spare Threshold:          10%\nPercentage Used:                    112%\n\
                    Data Units Written:                 23,456,789 [12.0 TB]\nPower On Hours:                     12,345\n\
                    Media and Data Integrity Errors:    3\n";
        let r = from_pve(&json!({"health": "FAILED!", "type": "text", "text": text, "wearout": 0}), 0);
        assert_eq!(r.protocol, Protocol::Nvme);
        assert_eq!(r.passed, Some(false));
        assert_eq!(r.critical_warning, Some(4));
        assert_eq!(r.temperature, Some(41.0));
        assert_eq!(r.wear_used, Some(112.0));
        assert_eq!(r.bytes_written, Some(23_456_789 * 512_000));
        assert_eq!(r.power_on_hours, Some(12345));
        let codes: Vec<String> =
            assess(&r, &Context { media: Media::Nvme, hdd_temp_limit: 50.0, ..Default::default() })
                .into_iter()
                .map(|f| f.code)
                .collect();
        for c in ["smart_failed", "critical_warning", "spare", "media_errors", "wear"] {
            assert!(codes.contains(&c.to_string()), "{c} missing from {codes:?}");
        }
    }

    #[test]
    fn parses_proxmox_sas_text() {
        let text = "Current Drive Temperature:     30 C\nDrive Trip Temperature:        68 C\n\
                    Accumulated start-stop cycles:  120\nElements in grown defect list: 2\n";
        let r = from_pve(&json!({"health": "OK", "type": "text", "text": text}), 0);
        assert_eq!(r.protocol, Protocol::Scsi);
        assert_eq!(r.passed, Some(true));
        assert_eq!(r.temperature, Some(30.0));
        assert_eq!(r.temp_critical, Some(68.0));
        assert_eq!(r.grown_defects, Some(2));
    }

    #[test]
    fn numbers_with_noise() {
        assert_eq!(first_uint("41234h+05m+12.345s"), Some(41234));
        assert_eq!(first_uint("34 (Min/Max 20/46)"), Some(34));
        assert_eq!(first_uint("12,345,678 [6.32 TB]"), Some(12_345_678));
        assert_eq!(first_uint("1,2"), Some(1));
        assert_eq!(first_uint("none"), None);
        assert_eq!(group_digits(1234567), "1,234,567");
        assert_eq!(group_digits(999), "999");
    }

    #[test]
    fn assessment_reads_trends_and_context() {
        let r = from_smartctl(&wd_red(), 0);
        let mut ctx = Context { media: Media::Hdd, hdd_temp_limit: 50.0, in_md: true, ..Default::default() };
        let f = assess(&r, &ctx);
        assert_eq!(verdict(&f), Level::Warning);
        let by = |code: &str| f.iter().find(|x| x.code == code).cloned();
        assert_eq!(by("pending").unwrap().level, Level::Warning);
        assert_eq!(by("reallocated").unwrap().level, Level::Watch);
        assert_eq!(by("crc").unwrap().level, Level::Info);
        assert_eq!(by("link_speed").unwrap().level, Level::Info);
        assert_eq!(by("erc").unwrap().level, Level::Info);
        assert!(by("smr").is_none());
        // Growing counters escalate.
        ctx.trends =
            Trends { days: 30.0, reallocated: Some(4), crc: Some(2), load_cycles: Some(30_000), ..Default::default() };
        let f = assess(&r, &ctx);
        let by = |code: &str| f.iter().find(|x| x.code == code).cloned();
        assert_eq!(by("reallocated").unwrap().level, Level::Warning);
        assert_eq!(by("crc").unwrap().level, Level::Watch);
        assert!(by("load_cycle_rate").is_some());
        // A failed self-test makes the disk failing.
        ctx.last_test = Some(r.self_tests.log[1].clone());
        let f = assess(&r, &ctx);
        assert_eq!(verdict(&f), Level::Failing);
        assert!(f[0].detail.contains("123456789"));
    }

    #[test]
    fn temperature_limits_follow_media() {
        let mut r = Report { temperature: Some(55.0), ..Default::default() };
        let hdd = Context { media: Media::Hdd, hdd_temp_limit: 50.0, ..Default::default() };
        assert_eq!(assess(&r, &hdd)[0].code, "temperature");
        let ssd = Context { media: Media::Ssd, hdd_temp_limit: 50.0, ..Default::default() };
        assert!(assess(&r, &ssd).is_empty());
        r.temp_warning = Some(52.0);
        assert_eq!(assess(&r, &ssd)[0].code, "temperature");
        r.temp_critical = Some(54.0);
        assert_eq!(assess(&r, &ssd)[0].level, Level::Failing);
    }

    #[test]
    fn healthy_disk_is_ok() {
        let r = Report {
            passed: Some(true),
            smart_available: Some(true),
            smart_enabled: Some(true),
            temperature: Some(33.0),
            reallocated: Some(0),
            pending: Some(0),
            power_on_hours: Some(1000),
            ..Default::default()
        };
        let f = assess(&r, &Context { media: Media::Hdd, hdd_temp_limit: 50.0, ..Default::default() });
        assert!(f.is_empty(), "{f:?}");
        assert_eq!(verdict(&f), Level::Ok);
    }

    #[test]
    fn learns_disks_that_sleep_between_readings() {
        // One new start can be a coincidence; two readings in a row can't.
        let s = SpinDown::default().next(Some(10), Some(11));
        assert!(!s.sleeps);
        let s = s.next(Some(11), Some(12));
        assert!(s.sleeps);
        // Read once a day, it still wakes in between.
        let s = s.next(Some(12), Some(15));
        assert!(s.sleeps);
        // An unknown count changes nothing; no new start means it stayed awake.
        assert_eq!(s.next(None, Some(15)), s);
        assert_eq!(s.next(Some(15), Some(15)), SpinDown::default());
        let v = json!({"health": "PASSED", "type": "ata", "attributes": [
            {"id": "  4", "name": "Start_Stop_Count", "value": 99, "normalized": 99, "worst": 99, "threshold": 0, "fail": "-", "flags": "-O--CK", "raw": "1234"}
        ]});
        assert_eq!(from_pve(&v, 0).start_stops, Some(1234));
        let sas = from_pve(&json!({"health": "OK", "type": "text", "text": "Accumulated start-stop cycles: 120\n"}), 0);
        assert_eq!((sas.start_stops, sas.power_cycles), (Some(120), Some(120)));
    }

    #[test]
    fn smr_models() {
        assert!(known_smr("WDC WD40EFAX-68JH4N0"));
        assert!(!known_smr("WDC WD80EFAX-68KNBN0"));
        assert!(!known_smr("WDC WD40EFRX-68N32N0"));
        assert!(known_smr("ST8000DM004-2CX188"));
    }

    #[test]
    fn outcome_codes() {
        assert_eq!(ata_outcome(0), TestOutcome::Passed);
        assert_eq!(ata_outcome(0x10), TestOutcome::Aborted);
        assert_eq!(ata_outcome(0x21), TestOutcome::Interrupted);
        assert_eq!(ata_outcome(0x79), TestOutcome::Failed);
        assert_eq!(ata_outcome(0xF9), TestOutcome::Running);
        assert_eq!(nvme_outcome(0), TestOutcome::Passed);
        assert_eq!(nvme_outcome(7), TestOutcome::Failed);
        assert_eq!(nvme_outcome(2), TestOutcome::Interrupted);
    }
}
