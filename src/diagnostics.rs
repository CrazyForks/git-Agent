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
    if let Some(parent) = branch_switch_log_path().parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(branch_switch_log_path(), []);
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
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
        .join("branch-switch.log")
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

pub fn window_drag_log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
        .join("window-drag.log")
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
