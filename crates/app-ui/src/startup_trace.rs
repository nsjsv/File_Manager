use std::env;
use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const TRACE_ENV: &str = "FILE_MANAGER_STARTUP_TRACE";

static STARTUP_TRACE: OnceLock<Option<StartupTrace>> = OnceLock::new();

struct StartupTrace {
    started_at: Instant,
    emitted_once_labels: Mutex<Vec<&'static str>>,
}

pub(crate) fn init_from_env() {
    let _ = STARTUP_TRACE.set(trace_from_env());
}

pub(crate) fn mark(label: &'static str) {
    if let Some(trace) = current_trace() {
        trace.emit(label);
    }
}

pub(crate) fn mark_once(label: &'static str) {
    if let Some(trace) = current_trace() {
        trace.emit_once(label);
    }
}

fn current_trace() -> Option<&'static StartupTrace> {
    STARTUP_TRACE.get_or_init(trace_from_env).as_ref()
}

fn trace_from_env() -> Option<StartupTrace> {
    let raw_value = env::var_os(TRACE_ENV)?;
    if !env_value_enables_trace(raw_value) {
        return None;
    }

    Some(StartupTrace {
        started_at: Instant::now(),
        emitted_once_labels: Mutex::new(Vec::new()),
    })
}

fn env_value_enables_trace(raw_value: OsString) -> bool {
    let normalized_value = raw_value.to_string_lossy().trim().to_ascii_lowercase();
    !matches!(normalized_value.as_str(), "" | "0" | "false" | "no" | "off")
}

impl StartupTrace {
    fn emit(&self, label: &'static str) {
        let elapsed_ms = self.started_at.elapsed().as_secs_f64() * 1000.0;
        eprintln!("startup.{label}={elapsed_ms:.1}ms");
    }

    fn emit_once(&self, label: &'static str) {
        let Ok(mut emitted_once_labels) = self.emitted_once_labels.lock() else {
            return;
        };

        if emitted_once_labels.contains(&label) {
            return;
        }

        emitted_once_labels.push(label);
        drop(emitted_once_labels);
        self.emit(label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_env_accepts_common_truthy_values() {
        assert!(env_value_enables_trace(OsString::from("1")));
        assert!(env_value_enables_trace(OsString::from("true")));
        assert!(env_value_enables_trace(OsString::from("yes")));
    }

    #[test]
    fn trace_env_rejects_common_disabled_values() {
        assert!(!env_value_enables_trace(OsString::from("")));
        assert!(!env_value_enables_trace(OsString::from("0")));
        assert!(!env_value_enables_trace(OsString::from("false")));
        assert!(!env_value_enables_trace(OsString::from("no")));
        assert!(!env_value_enables_trace(OsString::from("off")));
    }
}
