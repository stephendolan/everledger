use crate::{Failure, Result, time::UtcInstant};
use std::{
    cell::Cell,
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
        UtcInstant::from_microseconds(jiff::Timestamp::now().as_microsecond())
            .unwrap_or(UtcInstant::UNIX_EPOCH)
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
    elapsed: Cell<f64>,
    wall: Cell<UtcInstant>,
    sleep_log: Option<std::path::PathBuf>,
}
impl VirtualClock {
    pub fn new(origin: UtcInstant) -> Self {
        Self {
            origin,
            elapsed: Cell::new(0.0),
            wall: Cell::new(origin),
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
            elapsed: Cell::new(0.0),
            wall: Cell::new(origin),
            sleep_log: log,
        }))
    }
}
impl Clock for VirtualClock {
    fn now(&self) -> UtcInstant {
        self.wall.get()
    }
    fn monotonic(&self) -> f64 {
        self.elapsed.get()
    }
    fn sleep(&self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(Failure::error("invalid sleep duration"));
        }
        let elapsed = self.elapsed.get() + seconds;
        let micros = (elapsed * 1e6).round_ties_even();
        if !micros.is_finite() || micros >= i64::MAX as f64 {
            return Err(Failure::error("virtual clock overflow"));
        }
        let wall = self.origin.add_microseconds(micros as i64)?;
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
        self.elapsed.set(elapsed);
        self.wall.set(wall);
        Ok(())
    }
}
