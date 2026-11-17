use super::write_at;
use crate::{
    Failure, Result,
    clock::{Clock, SystemClock, VirtualClock},
    document::Document,
    time::UtcInstant,
};
use signal_hook::{
    consts::SIGINT,
    iterator::{Handle, Signals},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
struct EndBySigint;
impl Drop for EndBySigint {
    fn drop(&mut self) {
        let _ = signal_hook::low_level::emulate_default_handler(SIGINT);
    }
}
fn default_sigint() -> Result<Arc<AtomicBool>> {
    static ENABLED: OnceLock<Result<Arc<AtomicBool>>> = OnceLock::new();
    ENABLED
        .get_or_init(|| {
            let enabled = Arc::new(AtomicBool::new(true));
            signal_hook::flag::register_conditional_default(SIGINT, Arc::clone(&enabled))?;
            Ok(enabled)
        })
        .clone()
}
struct Snapshot {
    attempt: Document,
    pending: Document,
    now: UtcInstant,
}
pub(super) struct Guard {
    snapshot: Arc<Mutex<Option<Snapshot>>>,
    interrupted: Arc<AtomicBool>,
    flag_id: signal_hook::SigId,
    default_enabled: Arc<AtomicBool>,
    dir: PathBuf,
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}
impl Guard {
    pub fn new(dir: &Path) -> Result<Self> {
        let default_enabled = default_sigint()?;
        let interrupted = Arc::new(AtomicBool::new(false));
        let flag_id = signal_hook::flag::register(SIGINT, Arc::clone(&interrupted))?;
        let mut signals = Signals::new([SIGINT])?;
        let handle = signals.handle();
        let snapshot: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));
        let state = Arc::clone(&snapshot);
        let directory = PathBuf::from(dir);
        let dir = directory.clone();
        #[cfg(feature = "parity-seams")]
        let virtual_clock = std::env::var_os("EVERLEDGER_SEAM_NOW").is_some();
        #[cfg(not(feature = "parity-seams"))]
        let virtual_clock = false;
        let thread = std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _termination = EndBySigint;
                let mut state = state.lock().ok();
                if let Some(s) = state.as_mut().and_then(|state| state.as_mut()) {
                    let now = if virtual_clock {
                        s.now
                    } else {
                        SystemClock::default().now()
                    };
                    let _ = super::finish_attempt(&mut s.attempt, now);
                    let clock = VirtualClock::new(now);
                    let _ = s.attempt.set("failed", true);
                    let _ = s.attempt.set("status", Document::Null);
                    let _ = s
                        .attempt
                        .set("error", crate::human::refresh::sync_failed(None));
                    let _ = write_at(&dir, "attempt.json", &s.attempt, &clock);
                    let _ = write_at(&dir, "pending.json", &s.pending, &clock);
                }
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "{}",
                    crate::human::refresh::interrupted()
                );
            }
        });
        default_enabled.store(false, Ordering::SeqCst);
        Ok(Self {
            snapshot,
            interrupted,
            flag_id,
            default_enabled,
            dir: directory,
            handle,
            thread: Some(thread),
        })
    }
    fn wait_if_interrupted(&self) {
        while self.interrupted.load(Ordering::SeqCst) {
            std::thread::park();
        }
    }
    pub fn write(&self, name: &str, doc: &Document, clock: &dyn Clock) -> Result<()> {
        self.wait_if_interrupted();
        let mut state = self
            .snapshot
            .lock()
            .map_err(|_| Failure::error("ForeverDB interrupt state unavailable"))?;
        write_at(&self.dir, name, doc, clock)?;
        if let Some(state) = state.as_mut() {
            match name {
                "attempt.json" => state.attempt = doc.clone(),
                "pending.json" => state.pending = doc.clone(),
                _ => {}
            }
        }
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        self.wait_if_interrupted();
        let _state = self
            .snapshot
            .lock()
            .map_err(|_| Failure::error("ForeverDB interrupt state unavailable"))?;
        super::remove(&self.dir, name)
    }
    pub fn update(&self, attempt: &Document, pending: &Document, now: UtcInstant) {
        self.wait_if_interrupted();
        if let Ok(mut state) = self.snapshot.lock() {
            *state = Some(Snapshot {
                attempt: attempt.clone(),
                pending: pending.clone(),
                now,
            });
        }
    }
    pub fn clear(&self) {
        self.wait_if_interrupted();
        if let Ok(mut s) = self.snapshot.lock() {
            *s = None;
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.default_enabled.store(true, Ordering::SeqCst);
        self.wait_if_interrupted();
        signal_hook::low_level::unregister(self.flag_id);
        self.handle.close();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
