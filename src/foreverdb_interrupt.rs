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
struct EndBySigint<'a>(std::sync::MutexGuard<'a, Option<Snapshot>>);
impl Drop for EndBySigint<'_> {
    fn drop(&mut self) {
        let _ = signal_hook::low_level::emulate_default_handler(SIGINT);
        std::process::exit(130);
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
                let mut termination = EndBySigint(
                    state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                if let Some(s) = termination.0.as_mut() {
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
    fn lock_snapshot(&self) -> Result<std::sync::MutexGuard<'_, Option<Snapshot>>> {
        loop {
            self.wait_if_interrupted();
            let state = self
                .snapshot
                .lock()
                .map_err(|_| Failure::error("ForeverDB interrupt state unavailable"))?;
            if !self.interrupted.load(Ordering::SeqCst) {
                return Ok(state);
            }
            drop(state);
        }
    }
    pub fn write(&self, name: &str, doc: &Document, clock: &dyn Clock) -> Result<()> {
        let mut state = self.lock_snapshot()?;
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
    pub fn clear_retry(
        &self,
        pending: &mut crate::http::Payload<super::Pending>,
        clock: &dyn Clock,
    ) -> Result<()> {
        let mut state = self.lock_snapshot()?;
        if pending.document().get("retry").is_none() {
            return Ok(());
        }
        pending.remove("retry")?;
        if let Some(snapshot) = state.as_mut() {
            snapshot.pending = pending.document().clone();
            snapshot.now = clock.now();
        }
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        let _state = self.lock_snapshot()?;
        super::remove(&self.dir, name)
    }
    pub fn update(&self, attempt: &Document, pending: &Document, now: UtcInstant) {
        if let Ok(mut state) = self.lock_snapshot() {
            *state = Some(Snapshot {
                attempt: attempt.clone(),
                pending: pending.clone(),
                now,
            });
        }
    }
    pub fn clear(&self) {
        if let Ok(mut s) = self.lock_snapshot() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::parse, http::Payload, time::instant};
    use std::{
        os::unix::process::ExitStatusExt,
        process::Command,
        time::{Duration, Instant},
    };

    fn retry_response_child(dir: &Path) -> Result<()> {
        use crate::{
            config::Config,
            http::{Http, Request, Response},
            time::UtcInstant,
        };
        struct ResponseClock {
            returned: AtomicBool,
            signaled: AtomicBool,
            now: UtcInstant,
        }
        impl Clock for ResponseClock {
            fn now(&self) -> UtcInstant {
                if self.returned.load(Ordering::SeqCst)
                    && !self.signaled.swap(true, Ordering::SeqCst)
                {
                    assert!(
                        Command::new("kill")
                            .args(["-INT", &std::process::id().to_string()])
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                self.now
            }
            fn monotonic(&self) -> f64 {
                0.0
            }
            fn sleep(&self, _: f64) -> Result<()> {
                Ok(())
            }
        }
        struct InvalidResponse<'a>(&'a ResponseClock);
        impl Http for InvalidResponse<'_> {
            fn request(&self, _: &Request) -> Result<Response> {
                self.0.returned.store(true, Ordering::SeqCst);
                Ok(Response {
                    body: b"not JSON".to_vec(),
                    headers: vec![],
                })
            }
        }
        let config = Config::from_values(&std::collections::HashMap::from([
            (
                "EVERLEDGER_DATA_DIR".into(),
                dir.to_string_lossy().into_owned(),
            ),
            ("EVERLEDGER_FOREVERDB_API_KEY".into(), "fixture".into()),
            (
                "EVERLEDGER_SEAM_FOREVERDB_URL".into(),
                "http://127.0.0.1:1".into(),
            ),
        ]))?;
        let cache = dir.join("vendors/foreverdb.net");
        std::fs::create_dir_all(&cache)?;
        std::fs::write(
            cache.join("pending.json"),
            r#"{"format":2,"sync_id":"fixture","next_offset":0,"vendor_ids":[],"details":{},"metadata":null,"total_requests":0,"retry":{"not_before":"2026-11-15T11:00:00Z"}}"#,
        )?;
        let clock = ResponseClock {
            returned: AtomicBool::new(false),
            signaled: AtomicBool::new(false),
            now: instant("2026-11-15T12:00:00Z")?,
        };
        let http = InvalidResponse(&clock);
        let catalogue = crate::catalogue::CatalogueStore::default();
        let diagnostics = crate::diagnostics::WriterDiagnostics::<Vec<u8>>::default();
        let context = crate::Context {
            config: &config,
            clock: &clock,
            http: &http,
            catalogue: &catalogue,
            diagnostics: &diagnostics,
        };
        let _ = super::super::sync(&context);
        panic!("signal did not terminate the sync");
    }

    #[test]
    fn signal_child() -> Result<()> {
        let Some(dir) = std::env::var_os("EVERLEDGER_SIGNAL_TEST_DIR") else {
            return Ok(());
        };
        let dir = PathBuf::from(dir);
        if std::env::var("EVERLEDGER_SIGNAL_RESPONSE_TEST").as_deref() == Ok("1") {
            return retry_response_child(&dir);
        }
        let clock = VirtualClock::new(instant("2026-11-15T12:00:00Z")?);
        let guard = Arc::new(Guard::new(&dir)?);
        let attempt = parse(
            r#"{"attempted_at":"2026-11-15T12:00:00Z","requests":1,"spends":[],"failed":false,"status":null}"#,
        )?;
        let mut pending: Payload<super::super::Pending> = Payload::decode(
            parse(
                r#"{"sync_id":"fixture","next_offset":0,"vendor_ids":[],"details":{},"metadata":null,"total_requests":1,"retry":{"not_before":"2026-11-15T12:01:00Z"}}"#,
            )?,
            "test checkpoint",
        )?;
        guard.update(&attempt, pending.document(), clock.now());
        guard.clear_retry(&mut pending, &clock)?;
        let blocked = guard.snapshot.lock().unwrap();
        let writer = Arc::clone(&guard);
        let (queued, waiting) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut overwrite = attempt;
            overwrite.set("requests", 9i64).unwrap();
            queued.send(()).unwrap();
            let _ = writer.write("attempt.json", &overwrite, &clock);
        });
        waiting.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            Command::new("kill")
                .args(["-INT", &std::process::id().to_string()])
                .status()?
                .success()
        );
        while !guard.interrupted.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        drop(blocked);
        loop {
            std::thread::park();
        }
    }

    #[test]
    fn signal_keeps_reserved_spend_and_cleared_retry_under_contention() -> Result<()> {
        for iteration in 0..8 {
            let response = iteration >= 4;
            let dir = std::env::temp_dir().join(format!(
                "everledger-signal-{}-{iteration}",
                std::process::id()
            ));
            std::fs::create_dir(&dir)?;
            let mut child = Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "foreverdb::interrupt::tests::signal_child",
                    "--nocapture",
                ])
                .env("EVERLEDGER_SIGNAL_TEST_DIR", &dir)
                .env(
                    "EVERLEDGER_SIGNAL_RESPONSE_TEST",
                    if response { "1" } else { "" },
                )
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                if Instant::now() > deadline {
                    child.kill()?;
                    panic!("signal handler did not terminate");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.signal(), Some(SIGINT));
            let cache = if response {
                dir.join("vendors/foreverdb.net")
            } else {
                dir.clone()
            };
            let attempt = parse(&std::fs::read_to_string(cache.join("attempt.json"))?)?;
            assert_eq!(attempt.get("requests"), Some(&1i64.into()));
            assert_eq!(attempt.get("failed"), Some(&true.into()));
            assert_eq!(attempt.get("error"), Some(&"ForeverDB sync failed".into()));
            let pending = parse(&std::fs::read_to_string(cache.join("pending.json"))?)?;
            assert!(pending.get("retry").is_none());
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}
