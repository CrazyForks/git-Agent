use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TRACE_WINDOW_MS: u64 = 10 * 60 * 1_000;

static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_TRACE_ID: AtomicU64 = AtomicU64::new(0);
static ACTIVE_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_EXPIRES_MS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PROBE_FLUSHED_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PROBE_X_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PROBE_Y_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PROBE_AREA: AtomicU64 = AtomicU64::new(0);
static WINDOW_SIZE_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_SIZE_PROBE_FLUSHED_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_SIZE_PROBE_WIDTH_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_SIZE_PROBE_HEIGHT_BITS: AtomicU64 = AtomicU64::new(0);

pub struct TraceSpan {
    event: &'static str,
    fields: String,
    started: Instant,
}

impl Drop for TraceSpan {
    fn drop(&mut self) {
        trace(
            &format!("{}.end", self.event),
            &format!(
                "{} elapsed_ms={}",
                self.fields,
                self.started.elapsed().as_millis()
            ),
        );
    }
}

pub fn begin_branch_switch(fields: &str) -> u64 {
    let trace_id = NEXT_TRACE_ID.fetch_add(1, Ordering::Relaxed);
    let now = epoch_ms();
    ACTIVE_TRACE_ID.store(trace_id, Ordering::Release);
    ACTIVE_STARTED_MS.store(now, Ordering::Release);
    ACTIVE_EXPIRES_MS.store(now.saturating_add(TRACE_WINDOW_MS), Ordering::Release);

    let _guard = write_lock();
    let path = branch_switch_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, []);
    append_line(trace_id, now, "branch.request", fields);
    trace_id
}

pub fn trace(event: &str, fields: &str) {
    let trace_id = ACTIVE_TRACE_ID.load(Ordering::Acquire);
    if trace_id == 0 {
        return;
    }
    let now = epoch_ms();
    if now > ACTIVE_EXPIRES_MS.load(Ordering::Acquire) {
        return;
    }
    let _guard = write_lock();
    append_line(trace_id, now, event, fields);
}

pub fn span(event: &'static str, fields: impl Into<String>) -> TraceSpan {
    let fields = fields.into();
    trace(&format!("{event}.start"), &fields);
    TraceSpan {
        event,
        fields,
        started: Instant::now(),
    }
}

pub fn finish_branch_switch(outcome: &str, fields: &str) {
    trace(
        "branch.finish",
        &format!("outcome={} {}", clean(outcome), clean(fields)),
    );
    ACTIVE_EXPIRES_MS.store(epoch_ms().saturating_add(30_000), Ordering::Release);
}

pub fn branch_switch_log_path() -> PathBuf {
    daily_log_path("branch-switch")
}

/// Temporary drag diagnostics. The press path writes only atomics after dispatching the native
/// command; formatting, locking, and file I/O are delayed until the button has been released.
pub fn record_window_drag_probe(x: f32, y: f32, area: u8) {
    WINDOW_DRAG_PROBE_X_BITS.store(x.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_PROBE_Y_BITS.store(y.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_PROBE_AREA.store(area as u64, Ordering::Release);
    WINDOW_DRAG_PROBE_SEQUENCE.fetch_add(1, Ordering::Release);
}

pub fn flush_window_drag_probe() {
    let sequence = WINDOW_DRAG_PROBE_SEQUENCE.load(Ordering::Acquire);
    let flushed = WINDOW_DRAG_PROBE_FLUSHED_SEQUENCE.load(Ordering::Acquire);
    if sequence == flushed {
        return;
    }
    WINDOW_DRAG_PROBE_FLUSHED_SEQUENCE.store(sequence, Ordering::Release);

    let x = f32::from_bits(WINDOW_DRAG_PROBE_X_BITS.load(Ordering::Acquire) as u32);
    let y = f32::from_bits(WINDOW_DRAG_PROBE_Y_BITS.load(Ordering::Acquire) as u32);
    let area = match WINDOW_DRAG_PROBE_AREA.load(Ordering::Acquire) {
        1 => "title-layer",
        2 => "source-gap",
        3 => "resize-north",
        4 => "resize-south",
        5 => "resize-east",
        6 => "resize-west",
        7 => "resize-north-east",
        8 => "resize-south-east",
        9 => "resize-north-west",
        10 => "resize-south-west",
        _ => "title-other",
    };
    let line = format!(
        "epoch_ms={} pid={} sequence={} event=window_drag.pointer_press x={x:.1} y={y:.1} area={area}\n",
        epoch_ms(),
        std::process::id(),
        sequence,
    );
    let _guard = write_lock();
    let path = window_drag_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Records the most recent client-area size without performing file I/O in the
/// resize path. The value is written after the mouse is released together with
/// the drag probe, making failed native resize hit tests distinguishable from a
/// resize that is clamped by the configured minimum size.
pub fn record_window_size_probe(width: f32, height: f32) {
    let width_bits = width.to_bits() as u64;
    let height_bits = height.to_bits() as u64;
    let previous_width = WINDOW_SIZE_PROBE_WIDTH_BITS.swap(width_bits, Ordering::Relaxed);
    let previous_height = WINDOW_SIZE_PROBE_HEIGHT_BITS.swap(height_bits, Ordering::Relaxed);
    if previous_width != width_bits || previous_height != height_bits {
        WINDOW_SIZE_PROBE_SEQUENCE.fetch_add(1, Ordering::Release);
    }
}

pub fn flush_window_size_probe() {
    let sequence = WINDOW_SIZE_PROBE_SEQUENCE.load(Ordering::Acquire);
    let flushed = WINDOW_SIZE_PROBE_FLUSHED_SEQUENCE.load(Ordering::Acquire);
    if sequence == flushed {
        return;
    }
    WINDOW_SIZE_PROBE_FLUSHED_SEQUENCE.store(sequence, Ordering::Release);

    let width = f32::from_bits(WINDOW_SIZE_PROBE_WIDTH_BITS.load(Ordering::Acquire) as u32);
    let height = f32::from_bits(WINDOW_SIZE_PROBE_HEIGHT_BITS.load(Ordering::Acquire) as u32);
    append_window_drag_line(format!(
        "epoch_ms={} pid={} sequence={} event=window_resize.observed width={width:.1} height={height:.1}\n",
        epoch_ms(),
        std::process::id(),
        sequence,
    ));
}

pub fn window_drag_log_path() -> PathBuf {
    daily_log_path("window-drag")
}

pub fn merge_ai_log_path() -> PathBuf {
    daily_log_path("merge-ai")
}

/// Append one sanitized, single-line event for the standalone merge assistant. Callers must never
/// include credentials. Keeping this separate from the process lifecycle log makes an AI request
/// trace easy to inspect without exposing the API key passed only in memory.
pub fn merge_ai_trace(event: &str, fields: &str) {
    let _guard = write_lock();
    let path = merge_ai_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let fields = fields.replace(['\r', '\n'], "\\n");
        let _ = writeln!(file, "[{}] {event} {fields}", epoch_ms());
    }
}

pub fn daily_log_path(stem: &str) -> PathBuf {
    log_directory().join(format!("{stem}-{}.log", utc_date_stamp(SystemTime::now())))
}

fn append_window_drag_line(line: String) {
    let _guard = write_lock();
    let path = window_drag_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn log_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}

fn utc_date_stamp(now: SystemTime) -> String {
    let elapsed = now.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let days = (elapsed.as_secs() / 86_400) as i64;
    let (year, month, day) = civil_date_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_date_from_unix_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

fn append_line(trace_id: u64, now: u64, event: &str, fields: &str) {
    let started = ACTIVE_STARTED_MS.load(Ordering::Acquire);
    let line = format!(
        "epoch_ms={now} elapsed_ms={} trace={} pid={} thread={:?} event={} {}\n",
        now.saturating_sub(started),
        trace_id,
        std::process::id(),
        std::thread::current().id(),
        clean(event),
        clean(fields),
    );
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(branch_switch_log_path())
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn write_lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn clean(value: &str) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::civil_date_from_unix_days;

    #[test]
    fn unix_day_conversion_generates_stable_daily_log_dates() {
        assert_eq!(civil_date_from_unix_days(0), (1970, 1, 1));
        assert_eq!(civil_date_from_unix_days(20_312), (2025, 8, 12));
        assert_eq!(civil_date_from_unix_days(20_677), (2026, 8, 12));
    }
}
