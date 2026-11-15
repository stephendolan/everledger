use crate::{Failure, Result, time::UtcInstant};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub trait Clock {
    fn now(&self) -> UtcInstant;
    fn monotonic(&self) -> f64;
    fn sleep(&self, seconds: f64) -> Result<()>;
}
pub struct SystemClock {
    start: Instant,
}
impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}
impl Clock for SystemClock {
    fn now(&self) -> UtcInstant {
        UtcInstant(
            jiff::Timestamp::now()
                .round(jiff::Unit::Microsecond)
                .expect("system time in range"),
        )
    }
    fn monotonic(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    fn sleep(&self, seconds: f64) -> Result<()> {
        let d = Duration::try_from_secs_f64(seconds).map_err(|e| Failure::error(e.to_string()))?;
        std::thread::sleep(d);
        Ok(())
    }
}
pub struct VirtualClock {
    origin: UtcInstant,
    elapsed: Mutex<f64>,
    sleep_log: Option<std::path::PathBuf>,
}
impl VirtualClock {
    pub fn new(origin: UtcInstant) -> Self {
        Self {
            origin,
            elapsed: Mutex::new(0.0),
            sleep_log: None,
        }
    }
    #[cfg(feature = "parity-seams")]
    pub fn from_env() -> Result<Option<Self>> {
        let origin = std::env::var("EVERLEDGER_SEAM_NOW").ok();
        let log = std::env::var_os("EVERLEDGER_SEAM_SLEEP_LOG").map(Into::into);
        if origin.is_none() && log.is_none() {
            return Ok(None);
        }
        let origin = match origin {
            Some(s) => {
                if !s.ends_with('Z') {
                    return Err(Failure::usage("EVERLEDGER_SEAM_NOW requires UTC Z suffix"));
                }
                crate::time::instant(&s)?
            }
            None => SystemClock::default().now(),
        };
        Ok(Some(Self {
            origin,
            elapsed: Mutex::new(0.0),
            sleep_log: log,
        }))
    }
}
impl Clock for VirtualClock {
    fn now(&self) -> UtcInstant {
        self.origin
            .add_microseconds((*self.elapsed.lock().unwrap() * 1e6).round_ties_even() as i64)
            .expect("virtual time in range")
    }
    fn monotonic(&self) -> f64 {
        *self.elapsed.lock().unwrap()
    }
    fn sleep(&self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(Failure::error("invalid sleep duration"));
        }
        if let Some(path) = &self.sleep_log {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?,
                "{}",
                crate::primitives::float_repr(seconds)
            )?;
        }
        *self.elapsed.lock().unwrap() += seconds;
        Ok(())
    }
}
