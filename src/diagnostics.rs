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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Trace,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_TRACE_ID: AtomicU64 = AtomicU64::new(0);
static ACTIVE_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_EXPIRES_MS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_FLUSHED_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_PRESSED_MS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_X_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_Y_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_DECISION: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_SCREEN_WIDTH_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_MENU_CONTROLS_RIGHT_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_WINDOW_CONTROLS_LEFT_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_START_OUTER_X_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_START_OUTER_Y_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_RELEASE_OUTER_X_BITS: AtomicU64 = AtomicU64::new(0);
static WINDOW_DRAG_RELEASE_OUTER_Y_BITS: AtomicU64 = AtomicU64::new(0);

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

pub fn app_info(event: &str, fields: &str) {
    write_domain_event("app", LogLevel::Info, event, fields);
}

pub fn app_error(event: &str, fields: &str) {
    write_domain_event("app", LogLevel::Error, event, fields);
}

pub fn merge_tool_info(event: &str, fields: &str) {
    write_domain_event("merge-tool", LogLevel::Info, event, fields);
}

pub fn merge_tool_error(event: &str, fields: &str) {
    write_domain_event("merge-tool", LogLevel::Error, event, fields);
}

pub fn error_log_path() -> PathBuf {
    daily_log_path("error")
}

/// Temporary title-bar diagnostics requested for interactive validation. The press path performs
/// atomic stores only. Formatting, locking, and file I/O happen after the primary button is up.
#[allow(clippy::too_many_arguments)]
pub fn record_window_drag_probe(
    x: f32,
    y: f32,
    decision: u8,
    screen_width: f32,
    menu_controls_right: f32,
    window_controls_left: f32,
    outer_x: f32,
    outer_y: f32,
) {
    WINDOW_DRAG_PRESSED_MS.store(epoch_ms(), Ordering::Relaxed);
    WINDOW_DRAG_X_BITS.store(x.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_Y_BITS.store(y.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_DECISION.store(decision as u64, Ordering::Relaxed);
    WINDOW_DRAG_SCREEN_WIDTH_BITS.store(screen_width.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_MENU_CONTROLS_RIGHT_BITS
        .store(menu_controls_right.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_WINDOW_CONTROLS_LEFT_BITS
        .store(window_controls_left.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_START_OUTER_X_BITS.store(outer_x.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_START_OUTER_Y_BITS.store(outer_y.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_RELEASE_OUTER_X_BITS.store(outer_x.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_RELEASE_OUTER_Y_BITS.store(outer_y.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_SEQUENCE.fetch_add(1, Ordering::Release);
}

pub fn record_window_drag_release_position(outer_x: f32, outer_y: f32) {
    if WINDOW_DRAG_SEQUENCE.load(Ordering::Acquire)
        == WINDOW_DRAG_FLUSHED_SEQUENCE.load(Ordering::Acquire)
    {
        return;
    }
    WINDOW_DRAG_RELEASE_OUTER_X_BITS.store(outer_x.to_bits() as u64, Ordering::Relaxed);
    WINDOW_DRAG_RELEASE_OUTER_Y_BITS.store(outer_y.to_bits() as u64, Ordering::Release);
}

pub fn flush_window_drag_probe() {
    let sequence = WINDOW_DRAG_SEQUENCE.load(Ordering::Acquire);
    if sequence == WINDOW_DRAG_FLUSHED_SEQUENCE.load(Ordering::Acquire) {
        return;
    }
    WINDOW_DRAG_FLUSHED_SEQUENCE.store(sequence, Ordering::Release);

    let float = |value: &AtomicU64| f32::from_bits(value.load(Ordering::Acquire) as u32);
    let decision = match WINDOW_DRAG_DECISION.load(Ordering::Acquire) {
        1 => "title-drag",
        2 => "menu-controls-excluded",
        3 => "window-controls-excluded",
        4 => "stale-hit-map-excluded",
        5 => "source-gap-drag",
        _ => "unknown",
    };
    let start_x = float(&WINDOW_DRAG_START_OUTER_X_BITS);
    let start_y = float(&WINDOW_DRAG_START_OUTER_Y_BITS);
    let release_x = float(&WINDOW_DRAG_RELEASE_OUTER_X_BITS);
    let release_y = float(&WINDOW_DRAG_RELEASE_OUTER_Y_BITS);
    let now = epoch_ms();
    let line = format!(
        "[{now}] [TRACE] event=window_drag pid={} sequence={} decision={} hold_ms={} press_x={:.1} press_y={:.1} screen_width={:.1} menu_controls_right={:.1} window_controls_left={:.1} moved_x={:.1} moved_y={:.1}\n",
        std::process::id(),
        sequence,
        decision,
        epoch_ms().saturating_sub(WINDOW_DRAG_PRESSED_MS.load(Ordering::Acquire)),
        float(&WINDOW_DRAG_X_BITS),
        float(&WINDOW_DRAG_Y_BITS),
        float(&WINDOW_DRAG_SCREEN_WIDTH_BITS),
        float(&WINDOW_DRAG_MENU_CONTROLS_RIGHT_BITS),
        float(&WINDOW_DRAG_WINDOW_CONTROLS_LEFT_BITS),
        release_x - start_x,
        release_y - start_y,
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

pub fn window_drag_log_path() -> PathBuf {
    daily_log_path("window-drag")
}

pub fn merge_ai_log_path() -> PathBuf {
    daily_log_path("merge-ai")
}

pub fn repository_refresh_log_path() -> PathBuf {
    daily_log_path("repository-refresh")
}

pub fn repository_refresh_trace(event: &str, fields: &str) {
    write_domain_event(
        "repository-refresh",
        repository_refresh_level(fields),
        event,
        fields,
    );
}

fn repository_refresh_level(fields: &str) -> LogLevel {
    if fields.contains("outcome=error") || fields.contains("outcome=probe-error") {
        LogLevel::Error
    } else {
        LogLevel::Trace
    }
}

/// Append one sanitized, single-line event for the standalone merge assistant. Callers must never
/// include credentials. Keeping this separate from the process lifecycle log makes an AI request
/// trace easy to inspect without exposing the API key passed only in memory.
pub fn merge_ai_trace(event: &str, fields: &str) {
    write_domain_event("merge-ai", merge_ai_level(event), event, fields);
}

fn merge_ai_level(event: &str) -> LogLevel {
    if event.ends_with(".failed") || event.ends_with(".error") {
        LogLevel::Error
    } else if event.ends_with(".skipped")
        || event.ends_with(".rejected")
        || event.ends_with(".anchor_missing")
    {
        LogLevel::Warn
    } else {
        LogLevel::Trace
    }
}

fn write_domain_event(stem: &str, level: LogLevel, event: &str, fields: &str) {
    let _guard = write_lock();
    let now = epoch_ms();
    append_domain_event(daily_log_path(stem), now, level, event, fields);
    if level == LogLevel::Error {
        append_domain_event(
            error_log_path(),
            now,
            level,
            event,
            &format!("source={} {}", clean(stem), clean(fields)),
        );
    }
}

fn append_domain_event(path: PathBuf, now: u64, level: LogLevel, event: &str, fields: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(format_domain_event(now, level, event, fields).as_bytes());
    }
}

fn format_domain_event(now: u64, level: LogLevel, event: &str, fields: &str) -> String {
    format!(
        "[{now}] [{}] event={} {}\n",
        level.label(),
        clean(event),
        clean(fields)
    )
}

pub fn daily_log_path(stem: &str) -> PathBuf {
    log_directory().join(format!("{stem}-{}.log", utc_date_stamp(SystemTime::now())))
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
        "[{now}] [TRACE] elapsed_ms={} trace={} pid={} thread={:?} event={} {}\n",
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
    use super::{
        LogLevel, civil_date_from_unix_days, format_domain_event, merge_ai_level,
        repository_refresh_level,
    };

    #[test]
    fn unix_day_conversion_generates_stable_daily_log_dates() {
        assert_eq!(civil_date_from_unix_days(0), (1970, 1, 1));
        assert_eq!(civil_date_from_unix_days(20_312), (2025, 8, 12));
        assert_eq!(civil_date_from_unix_days(20_677), (2026, 8, 12));
    }

    #[test]
    fn log_levels_have_stable_uppercase_labels() {
        assert_eq!(LogLevel::Trace.label(), "TRACE");
        assert_eq!(LogLevel::Info.label(), "INFO");
        assert_eq!(LogLevel::Warn.label(), "WARN");
        assert_eq!(LogLevel::Error.label(), "ERROR");
    }

    #[test]
    fn structured_logs_classify_failures_and_sanitize_multiline_fields() {
        assert_eq!(merge_ai_level("request.failed"), LogLevel::Error);
        assert_eq!(merge_ai_level("parse.rejected"), LogLevel::Warn);
        assert_eq!(merge_ai_level("request.finished"), LogLevel::Trace);
        assert_eq!(
            repository_refresh_level("outcome=probe-error"),
            LogLevel::Error
        );
        assert_eq!(repository_refresh_level("outcome=success"), LogLevel::Trace);
        assert_eq!(
            format_domain_event(42, LogLevel::Error, "request.failed", "a\nb"),
            "[42] [ERROR] event=request.failed a b\n"
        );
    }
}
