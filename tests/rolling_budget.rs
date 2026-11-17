use everledger::{
    Context, Failure, Result,
    clock::Clock,
    config::Config,
    diagnostics::WriterDiagnostics,
    document::{Document, JsonMode, dumps, parse},
    foreverdb,
    http::{Http, Request, Response},
    time::{UtcInstant, instant},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};

struct TestClock {
    wall: Cell<UtcInstant>,
    elapsed: Cell<f64>,
    sleeps: RefCell<Vec<f64>>,
    block_on_sleep: RefCell<Option<PathBuf>>,
}
impl Clock for TestClock {
    fn now(&self) -> UtcInstant {
        self.wall.get()
    }
    fn monotonic(&self) -> f64 {
        self.elapsed.get()
    }
    fn sleep(&self, seconds: f64) -> Result<()> {
        if let Some(path) = self.block_on_sleep.borrow_mut().take() {
            std::fs::remove_file(&path)?;
            std::fs::create_dir(&path)?;
        }
        self.sleeps.borrow_mut().push(seconds);
        self.elapsed.set(self.elapsed.get() + seconds);
        Ok(())
    }
}
enum Injected {
    Retryable,
    RetryAfter(Document),
    Failure,
    AbruptExit,
}
struct Fixture {
    clock: Rc<TestClock>,
    data: RefCell<Document>,
    requests: RefCell<Vec<(String, f64)>>,
    scripts: RefCell<HashMap<usize, Injected>>,
    advance_after_request: Cell<i64>,
    block_checkpoint: RefCell<Option<PathBuf>>,
}
impl Http for Fixture {
    fn request(&self, request: &Request) -> Result<Response> {
        let count = {
            let mut requests = self.requests.borrow_mut();
            requests.push((request.url.clone(), self.clock.monotonic()));
            requests.len()
        };
        if let Some(script) = self.scripts.borrow_mut().remove(&count) {
            match script {
                Injected::AbruptExit => panic!("simulated abrupt process exit"),
                Injected::Retryable => {
                    return Err(Failure {
                        message: "secret-fixture".into(),
                        code: everledger::ExitCode::Unreachable,
                        body: Some(parse(
                            r#"{"status":502,"response":{"retryable":true,"echo":"secret-fixture"}}"#,
                        )?),
                    });
                }
                Injected::RetryAfter(delay) => {
                    return Err(Failure {
                        message: "upstream".into(),
                        code: everledger::ExitCode::Unreachable,
                        body: Some(Document::object([
                            ("status", 502i64.into()),
                            (
                                "response",
                                Document::object([
                                    ("retryable", true.into()),
                                    ("retry_after", delay),
                                ]),
                            ),
                        ])),
                    });
                }
                Injected::Failure => {
                    return Err(Failure {
                        message: "secret-fixture".into(),
                        code: everledger::ExitCode::Unreachable,
                        body: Some(parse(
                            r#"{"status":503,"response":{"echo":"secret-fixture"}}"#,
                        )?),
                    });
                }
            }
        }
        if let Some(path) = self.block_checkpoint.borrow_mut().take() {
            std::fs::create_dir(path)?;
        }
        let data = self.data.borrow();
        let body = if let Some((_, offset)) = request.url.split_once("offset=") {
            data.get("pages").and_then(Document::as_array).unwrap()
                [offset.parse::<usize>().unwrap() / 100]
                .clone()
        } else {
            data.get("details")
                .unwrap()
                .get(request.url.rsplit('/').next().unwrap())
                .unwrap()
                .clone()
        };
        self.clock.wall.set(
            self.clock
                .now()
                .add_microseconds(self.advance_after_request.get())?,
        );
        Ok(Response {
            body: dumps(&body, JsonMode::Stdout).into_bytes(),
            headers: vec![],
        })
    }
}
struct World {
    directory: PathBuf,
    config: Config,
    clock: Rc<TestClock>,
    http: Fixture,
    catalogue: everledger::catalogue::CatalogueStore,
    diagnostics: WriterDiagnostics<Vec<u8>>,
}
impl World {
    fn new(cap: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "everledger-window-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = Config::from_values(&HashMap::from([
            (
                "EVERLEDGER_DATA_DIR".into(),
                directory.to_string_lossy().into_owned(),
            ),
            (
                "EVERLEDGER_FOREVERDB_API_KEY".into(),
                "secret-fixture".into(),
            ),
            ("EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(), cap.into()),
            (
                "EVERLEDGER_SEAM_FOREVERDB_URL".into(),
                "http://127.0.0.1:1".into(),
            ),
        ]))
        .unwrap();
        let clock = Rc::new(TestClock {
            wall: Cell::new(instant("2026-11-15T12:00:00Z").unwrap()),
            elapsed: Cell::new(0.0),
            sleeps: RefCell::new(vec![]),
            block_on_sleep: RefCell::new(None),
        });
        let http = Fixture {
            clock: Rc::clone(&clock),
            data: RefCell::new(parse(include_str!("fixtures/foreverdb.json")).unwrap()),
            requests: RefCell::new(vec![]),
            scripts: RefCell::new(HashMap::new()),
            advance_after_request: Cell::new(0),
            block_checkpoint: RefCell::new(None),
        };
        Self {
            directory,
            config,
            clock,
            http,
            catalogue: Default::default(),
            diagnostics: Default::default(),
        }
    }
    fn context(&self) -> Context<'_> {
        Context {
            config: &self.config,
            clock: self.clock.as_ref(),
            http: &self.http,
            catalogue: &self.catalogue,
            diagnostics: &self.diagnostics,
        }
    }
    fn cap(&mut self, cap: &str) {
        self.config.foreverdb_daily_requests = Config::from_values(&HashMap::from([(
            "EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(),
            cap.into(),
        )]))
        .unwrap()
        .foreverdb_daily_requests;
    }
    fn advance(&self, seconds: i64) {
        self.clock.wall.set(
            self.clock
                .now()
                .add_microseconds(seconds * 1_000_000)
                .unwrap(),
        );
    }
    fn sync(&self) -> Result<foreverdb::SyncResult> {
        foreverdb::sync(&self.context())
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join("vendors/foreverdb.net").join(name)
    }
    fn disk(&self, name: &str) -> Document {
        parse(&std::fs::read_to_string(self.path(name)).unwrap()).unwrap()
    }
    fn write(&self, name: &str, data: &Document) {
        std::fs::create_dir_all(self.path(name).parent().unwrap()).unwrap();
        std::fs::write(self.path(name), dumps(data, JsonMode::Stdout)).unwrap();
    }
    fn spends(&self) -> Document {
        self.disk("attempt.json")
            .get("spends")
            .expect("spends")
            .clone()
    }
    fn crash_next(&self, offset: usize) {
        let at = self.http.requests.borrow().len() + offset;
        self.http
            .scripts
            .borrow_mut()
            .insert(at, Injected::AbruptExit);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sync()));
        assert!(result.is_err());
        assert!(self.disk("attempt.json").get("finished_at").is_none());
    }
}
impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn spend(requests: i64, finished_at: &str) -> Document {
    Document::object([
        ("requests", requests.into()),
        ("finished_at", finished_at.into()),
    ])
}

#[test]
fn window_spend_across_two_attempts_defers_at_cap() -> Result<()> {
    let world = World::new("12");
    assert_eq!(world.sync()?.view().requests, 6);
    world.advance(34 * 60);
    assert_eq!(world.sync()?.view().requests, 6);
    assert_eq!(
        world.spends(),
        Document::Array(vec![
            spend(6, "2026-11-15T12:00:00Z"),
            spend(6, "2026-11-15T12:34:00Z")
        ])
    );
    let before = std::fs::read(world.path("attempt.json"))?;
    let deferred = world.sync()?;
    assert_eq!(
        deferred.document(),
        &parse(
            r#"{"configured":true,"complete":true,"pending":false,"requests":0,"total_requests":6,"failed":false,"deferred":"daily sync limit","status":null,"error":null}"#
        )?
    );
    assert_eq!(world.http.requests.borrow().len(), 12);
    assert_eq!(std::fs::read(world.path("attempt.json"))?, before);
    Ok(())
}

#[test]
fn raised_cap_resumes_inside_window_with_remaining_budget() -> Result<()> {
    let mut world = World::new("2");
    assert!(!world.sync()?.view().complete);
    assert!(foreverdb::index(&world.context())?.is_none());
    world.advance(34 * 60);
    world.cap("6");
    let resumed = world.sync()?;
    assert_eq!(
        resumed.document(),
        &parse(
            r#"{"configured":true,"complete":true,"pending":false,"requests":4,"total_requests":6,"failed":false,"deferred":null,"status":null,"error":null}"#
        )?
    );
    assert_eq!(world.http.requests.borrow().len(), 6);
    let index = foreverdb::index(&world.context())?.unwrap();
    assert_eq!(
        foreverdb::lookup(2604, &index, &world.context())?
            .get("vendors")
            .and_then(Document::as_array)
            .unwrap()
            .len(),
        3
    );
    Ok(())
}

#[test]
fn window_expiry_restores_only_expired_spend_then_full_budget() -> Result<()> {
    let mut world = World::new("4");
    assert_eq!(world.sync()?.view().requests, 4);
    world.advance(3600);
    world.cap("6");
    assert_eq!(world.sync()?.view().requests, 2);
    world.advance(23 * 3600 - 1);
    assert_eq!(world.sync()?.view().requests, 0);
    world.advance(1);
    let partial = world.sync()?;
    assert_eq!(partial.view().requests, 4);
    assert!(!partial.view().complete);
    assert_eq!(
        world.spends(),
        Document::Array(vec![
            spend(2, "2026-11-15T13:00:00Z"),
            spend(4, "2026-11-16T12:00:00Z")
        ])
    );
    world.advance(3600);
    assert_eq!(world.sync()?.view().requests, 2);
    world.advance(86400);
    let full = world.sync()?;
    assert!(full.view().complete);
    assert_eq!(full.view().requests, 6);
    assert_eq!(
        world.spends(),
        Document::Array(vec![spend(6, "2026-11-17T13:00:00Z")])
    );
    Ok(())
}

#[test]
fn abrupt_exit_spend_uses_the_last_request_start() -> Result<()> {
    let world = World::new("3");
    world.http.advance_after_request.set(5 * 60 * 1_000_000);
    world.crash_next(3);
    world.advance(23 * 3600 + 55 * 60);
    assert_eq!(world.sync()?.view().requests, 0);
    assert_eq!(world.http.requests.borrow().len(), 3);
    Ok(())
}

#[test]
fn abrupt_exit_preserves_prior_window_spend() -> Result<()> {
    let world = World::new("8");
    assert_eq!(world.sync()?.view().requests, 6);
    world.crash_next(1);
    let resumed = world.sync()?;
    assert_eq!(resumed.view().requests, 1);
    assert!(!resumed.view().complete);
    assert_eq!(
        world.spends(),
        Document::Array(vec![
            spend(6, "2026-11-15T12:00:00Z"),
            spend(1, "2026-11-15T12:00:00Z"),
            spend(1, "2026-11-15T12:00:00Z")
        ])
    );
    assert_eq!(world.sync()?.view().requests, 0);
    Ok(())
}

#[test]
fn retry_502_counts_against_remaining_window_budget() -> Result<()> {
    let mut world = World::new("1");
    assert_eq!(world.sync()?.view().requests, 1);
    world.cap("2");
    world
        .http
        .scripts
        .borrow_mut()
        .insert(2, Injected::Retryable);
    let result = world.sync()?;
    assert_eq!(result.view().requests, 1);
    assert!(!result.view().complete);
    assert_eq!(world.http.requests.borrow().len(), 2);
    let deferred = world.sync()?;
    assert_eq!(deferred.view().requests, 0);
    assert!(!deferred.view().failed);
    assert_eq!(
        world.spends(),
        Document::Array(vec![
            spend(1, "2026-11-15T12:00:00Z"),
            spend(1, "2026-11-15T12:00:00Z")
        ])
    );
    Ok(())
}

#[test]
fn daily_refresh_refetches_details_without_revision_guarantee() -> Result<()> {
    let world = World::new("6");
    assert!(world.sync()?.view().complete);
    assert_eq!(world.sync()?.view().requests, 0);
    world.advance(86400);
    {
        let fixture = &mut *world.http.data.borrow_mut();
        let mut details = fixture.get("details").unwrap().clone();
        let mut vendor = details.get("8681").unwrap().clone();
        let mut data = vendor.get("data").unwrap().clone();
        data.set("name", "Changed Seller")?;
        vendor.set("data", data)?;
        details.set("8681", vendor)?;
        fixture.set("details", details)?;
    }
    assert!(world.sync()?.view().complete);
    assert_eq!(world.http.requests.borrow().len(), 12);
    assert!(dumps(&world.disk("index.json"), JsonMode::Stdout).contains("Changed Seller"));
    Ok(())
}

#[test]
fn changed_extraction_discards_checkpoint_and_recovers_next_day() -> Result<()> {
    let mut world = World::new("1");
    let mut first = parse(include_str!("fixtures/foreverdb-list.json"))?;
    let mut meta = first.get("meta").unwrap().clone();
    meta.set("total", 101i64)?;
    first.set("meta", meta)?;
    let second = parse(
        r#"{"data":[{"id":101}],"meta":{"build":"next-build","total":101,"limit":100,"offset":100,"next":null}}"#,
    )?;
    world
        .http
        .data
        .borrow_mut()
        .set("pages", Document::Array(vec![first, second]))?;
    assert!(!world.sync()?.view().complete);
    world.advance(86400);
    world.cap("36");
    assert!(world.sync()?.view().failed);
    assert!(foreverdb::index(&world.context())?.is_none());
    assert!(!world.path("pending.json").exists());
    assert!(world.sync()?.view().failed);
    world.advance(86400);
    *world.http.data.borrow_mut() = parse(include_str!("fixtures/foreverdb.json"))?;
    assert!(world.sync()?.view().complete);
    Ok(())
}

#[test]
fn pacing_and_failures_do_not_expose_credentials() -> Result<()> {
    let world = World::new("36");
    assert!(world.sync()?.view().complete);
    assert!(
        world
            .http
            .requests
            .borrow()
            .windows(2)
            .all(|pair| pair[1].1 - pair[0].1 >= 2.1 - 1e-9)
    );
    world.advance(86400);
    world.http.scripts.borrow_mut().insert(7, Injected::Failure);
    let failure = world.sync()?;
    assert!(failure.view().failed);
    assert_eq!(failure.view().status, Some(503));
    assert!(!format!("{failure:?}").contains("secret-fixture"));
    for entry in std::fs::read_dir(world.path("attempt.json").parent().unwrap())? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            assert!(!std::fs::read_to_string(path)?.contains("secret-fixture"));
        }
    }
    assert!(world.sync()?.view().complete);
    Ok(())
}

#[test]
fn plain_attempt_readers_ignore_spends_and_sync_requires_them() -> Result<()> {
    let world = World::new("36");
    for raw in [
        r#"{"attempted_at":"2026-11-15T12:00:00Z","requests":2,"failed":false}"#,
        r#"{"attempted_at":"2026-11-15T12:00:00Z","requests":2,"spends":[{"finished_at":"2026-11-15T12:00:00Z"}]}"#,
        r#"{"attempted_at":"2026-11-15T12:00:00Z","requests":2,"spends":[{"requests":1}]}"#,
        r#"{"attempted_at":"2026-11-15T12:00:00Z","spends":[]}"#,
    ] {
        let record = parse(raw)?;
        world.write("attempt.json", &record);
        let before = std::fs::read(world.path("attempt.json"))?;
        assert_eq!(
            foreverdb::read(&world.context(), "attempt.json")?,
            Some(record)
        );
        assert!(!foreverdb::attempt(&world.context())?.unwrap().failed);
        let (status, _) = everledger::vendors::status::backend_status(&world.context())?;
        assert!(!status.view().foreverdb.pending);
        assert_eq!(world.sync().unwrap_err().code, everledger::ExitCode::Error);
        assert_eq!(std::fs::read(world.path("attempt.json"))?, before);
        assert!(world.http.requests.borrow().is_empty());
    }
    Ok(())
}

#[test]
fn attempt_writes_prune_expired_spends_without_replenishing_the_fixed_budget() -> Result<()> {
    let world = World::new("6");
    let record = parse(
        r#"{"attempted_at":"2026-11-15T11:00:00Z","finished_at":"2026-11-15T11:00:00Z","requests":2,"spends":[{"requests":2,"finished_at":"2026-11-14T12:00:01Z"}]}"#,
    )?;
    world.write("attempt.json", &record);
    world.http.advance_after_request.set(1_000_000);
    let result = world.sync()?;
    assert_eq!(result.view().requests, 4);
    assert!(!result.view().complete);
    assert_eq!(
        world.spends(),
        Document::Array(vec![spend(4, "2026-11-15T12:00:04Z")])
    );
    let attempt = std::fs::read_to_string(world.path("attempt.json"))?;
    assert!(attempt.contains("\"sync_id\":"));
    assert!(attempt.find("\"sync_id\":").unwrap() < attempt.find("\"spends\":").unwrap());
    Ok(())
}

#[test]
fn numeric_delays_never_overflow_or_sleep_over_300() -> Result<()> {
    let huge = format!("1{}", "0".repeat(1000));
    for (raw, expected_wait) in [
        ("0", Some(60.0)),
        ("60", Some(60.0)),
        ("75", Some(75.0)),
        ("90.5", Some(91.0)),
        ("299.9", Some(300.0)),
        ("300", Some(300.0)),
        ("301", None),
        ("1e15", None),
        (huge.as_str(), None),
        ("Infinity", Some(60.0)),
        ("-Infinity", Some(60.0)),
        ("NaN", Some(60.0)),
        ("-5", Some(60.0)),
        (r#""soon""#, Some(60.0)),
        ("true", Some(60.0)),
        ("null", Some(60.0)),
        ("1e11", None),
        ("3e11", None),
        ("1e300", None),
        (format!("-{huge}").as_str(), Some(60.0)),
    ] {
        let world = World::new("36");
        world
            .http
            .scripts
            .borrow_mut()
            .insert(1, Injected::RetryAfter(parse(raw)?));
        let result = world.sync()?;
        assert!(!result.view().failed, "{raw}");
        match expected_wait {
            Some(wait) => {
                assert!(result.view().complete, "{raw}");
                assert_eq!(result.view().requests, 7, "{raw}");
                assert_eq!(
                    world
                        .clock
                        .sleeps
                        .borrow()
                        .iter()
                        .copied()
                        .find(|n| *n > 2.1),
                    Some(wait),
                    "{raw}"
                );
                assert_eq!(
                    world.disk("attempt.json").get("error"),
                    Some(&Document::Null)
                );
            }
            None => {
                assert!(result.view().pending, "{raw}");
                assert_eq!(result.view().requests, 1, "{raw}");
                assert_eq!(result.view().deferred, Some(foreverdb::Deferred::RetryWait));
                assert!(world.clock.sleeps.borrow().is_empty(), "{raw}");
                let pending = world.disk("pending.json");
                let stamp = pending
                    .get("retry")
                    .unwrap()
                    .get("not_before")
                    .unwrap()
                    .as_str()
                    .unwrap();
                assert!(instant(stamp)? > world.clock.now());
                if matches!(raw, "1e15" | "3e11" | "1e300") || raw == huge {
                    assert_eq!(stamp, "9999-12-31T23:59:59Z");
                }
                assert!(world.disk("attempt.json").get("error").is_none());
            }
        }
        assert!(
            world.clock.sleeps.borrow().iter().all(|n| *n <= 300.0),
            "{raw}"
        );
    }
    Ok(())
}

#[test]
fn long_wait_is_pending_and_resumes_after_not_before() -> Result<()> {
    for (advance, requests, wait) in [
        (0, 0, None),
        (1, 6, Some(300.0)),
        (300, 6, Some(60.0)),
        (301, 6, None),
        (302, 6, None),
    ] {
        let world = World::new("36");
        world
            .clock
            .wall
            .set(instant("2026-11-15T12:00:00.789012Z")?);
        world
            .http
            .scripts
            .borrow_mut()
            .insert(1, Injected::RetryAfter(301i64.into()));
        let first = world.sync()?;
        assert_eq!(first.view().requests, 1);
        assert_eq!(first.view().deferred, Some(foreverdb::Deferred::RetryWait));
        assert!(world.clock.sleeps.borrow().is_empty());
        assert_eq!(
            world
                .disk("pending.json")
                .get("retry")
                .unwrap()
                .get("not_before"),
            Some(&"2026-11-15T12:05:01.789012Z".into())
        );
        world.advance(advance);
        let resumed = world.sync()?;
        assert_eq!(resumed.view().requests, requests);
        assert_eq!(resumed.view().complete, requests > 0);
        assert_eq!(
            world
                .clock
                .sleeps
                .borrow()
                .iter()
                .copied()
                .find(|n| *n > 2.1),
            wait
        );
    }
    Ok(())
}

#[test]
fn sync_result_shape_on_success_failure_deferred_and_unconfigured() -> Result<()> {
    fn shape(result: &foreverdb::SyncResult) {
        let Document::Object(fields) = result.document() else {
            panic!("sync object");
        };
        assert_eq!(
            fields.iter().map(|(k, _)| k.as_ref()).collect::<Vec<_>>(),
            [
                "configured",
                "complete",
                "pending",
                "requests",
                "total_requests",
                "failed",
                "deferred",
                "status",
                "error"
            ]
        );
    }
    let mut world = World::new("36");
    world.config.foreverdb_api_key = "".into();
    let unconfigured = world.sync()?;
    shape(&unconfigured);
    assert_eq!(
        unconfigured.document(),
        &parse(
            r#"{"configured":false,"complete":false,"pending":false,"requests":0,"total_requests":0,"failed":false,"deferred":null,"status":null,"error":null}"#
        )?
    );
    world.config.foreverdb_api_key = "secret-fixture".into();
    world.http.scripts.borrow_mut().insert(1, Injected::Failure);
    let failed = world.sync()?;
    shape(&failed);
    assert_eq!(
        failed.document(),
        &parse(
            r#"{"configured":true,"complete":false,"pending":true,"requests":1,"total_requests":1,"failed":true,"deferred":null,"status":503,"error":"ForeverDB sync failed (HTTP 503)"}"#
        )?
    );
    let journal = world.disk("attempt.json");
    assert_eq!(journal.get("error"), failed.document().get("error"));
    let serialized = dumps(&journal, JsonMode::Stdout);
    assert!(serialized.rfind("\"finished_at\":").unwrap() < serialized.find("\"error\":").unwrap());
    world.cap("1");
    let deferred = world.sync()?;
    shape(&deferred);
    assert_eq!(deferred.view().requests, 0);
    assert_eq!(deferred.view().total_requests, 1);
    assert_eq!(deferred.view().status, Some(503));
    assert_eq!(deferred.view().error, failed.view().error);
    assert!(deferred.view().failed);
    world.cap("36");
    let complete = world.sync()?;
    shape(&complete);
    assert!(complete.view().complete);
    assert!(!complete.view().failed);
    assert_eq!(
        (complete.view().requests, complete.view().total_requests),
        (6, 7)
    );
    let serialized = dumps(&world.disk("attempt.json"), JsonMode::Stdout);
    assert!(serialized.find("\"error\":").unwrap() < serialized.rfind("\"finished_at\":").unwrap());
    Ok(())
}

#[test]
fn refresh_cached_returns_setup_failure_without_raising() -> Result<()> {
    let mut world = World::new("36");
    std::fs::create_dir_all(&world.directory)?;
    world.config.foreverdb_api_key = "".into();
    world.config.foreverdb_api_key_file = world.directory.clone();
    let (sources, result) = everledger::vendors::refresh_cached(&world.context())?;
    assert!(sources.is_empty());
    assert!(result.view().configured && result.view().failed);
    assert_eq!(
        (result.view().requests, result.view().total_requests),
        (0, 0)
    );
    assert_eq!(
        result.view().error.as_deref(),
        Some("ForeverDB API key file unreadable")
    );
    assert_eq!(
        everledger::human::refresh::lines(result.view()),
        [
            "catalogue refreshed",
            "ForeverDB sync: failed; 0 requests this refresh; ForeverDB API key file unreadable"
        ]
    );
    assert!(world.diagnostics.contents().is_empty());
    Ok(())
}

#[test]
fn status_uses_pending_existence_and_null_without_ninja_publication_times() -> Result<()> {
    let world = World::new("36");
    world.write("pending.json", &Document::Null);
    std::fs::write(world.path("pending.json"), "not JSON")?;
    let ninja_dir = world.directory.join("vendors/wowforever.ninja");
    std::fs::create_dir_all(&ninja_dir)?;
    for (id, published) in [(1, Document::Null), (2, "".into())] {
        let doc = Document::object([
            ("backend_url", world.config.vendor_url.clone().into()),
            ("vendors", Document::Array(vec![])),
            ("published_at", published),
        ]);
        std::fs::write(
            ninja_dir.join(format!("{id}.json")),
            dumps(&doc, JsonMode::Stdout),
        )?;
    }
    let (status, sources) = everledger::vendors::status::backend_status(&world.context())?;
    assert!(status.view().foreverdb.pending);
    assert_eq!(status.view().published_at, None);
    assert_eq!(sources[0].view().updated_at, None);
    Ok(())
}

#[test]
fn sync_checkpoint_write_failure_preserves_request_count() -> Result<()> {
    let world = World::new("36");
    *world.http.block_checkpoint.borrow_mut() = Some(world.path("pending.json"));
    let failed = world.sync()?;
    assert!(failed.view().failed);
    assert_eq!(
        (failed.view().requests, failed.view().total_requests),
        (1, 1)
    );
    assert_eq!(world.http.requests.borrow().len(), 1);
    assert_eq!(
        failed.view().error.as_deref(),
        Some("ForeverDB sync failed")
    );
    assert_eq!(
        world.disk("attempt.json").get("requests"),
        Some(&1i64.into())
    );

    let world = World::new("36");
    *world.clock.block_on_sleep.borrow_mut() = Some(world.path("attempt.json"));
    let failed = world.sync()?;
    assert!(failed.view().failed);
    assert_eq!(
        (failed.view().requests, failed.view().total_requests),
        (1, 1)
    );
    assert_eq!(
        world.http.requests.borrow().len(),
        1,
        "an unreserved second request must not be sent"
    );
    assert_eq!(
        failed.view().error.as_deref(),
        Some("ForeverDB sync failed")
    );

    let world = World::new("36");
    std::fs::create_dir_all(world.path("attempt.json"))?;
    let failed = world.sync()?;
    assert!(failed.view().failed);
    assert_eq!(
        (failed.view().requests, failed.view().total_requests),
        (0, 0)
    );
    assert!(world.http.requests.borrow().is_empty());
    Ok(())
}

#[test]
fn failed_sync_skips_unreadable_indexes_and_unrefreshable_ninja_items() -> Result<()> {
    let mut world = World::new("36");
    std::fs::create_dir_all(world.path("index.json"))?;
    let ninja_dir = world.directory.join("vendors/wowforever.ninja");
    std::fs::create_dir_all(&ninja_dir)?;
    std::fs::write(ninja_dir.join("2604.json"), "invalid JSON")?;
    let (sources, synced) = everledger::vendors::refresh_cached(&world.context())?;
    assert!(synced.view().failed);
    assert!(sources.is_empty());
    assert!(world.http.requests.borrow().is_empty());
    assert!(world.diagnostics.contents().is_empty());
    world.config.foreverdb_api_key = "".into();
    assert!(everledger::vendors::refresh_cached(&world.context()).is_err());
    Ok(())
}

struct RefreshHttp<'a>(&'a Fixture);
impl Http for RefreshHttp<'_> {
    fn request(&self, request: &Request) -> Result<Response> {
        if request.url.contains("/professions/") {
            return Ok(Response {
                body: br#"Recipes (2)<tr role="row"><td role="cell"><a href="/professions/alchemy/1-recipe/">Recipe</a></td><td role="cell"><span class="o">1</span></td><td role="cell">First</td><td role="cell"></td></tr><tr role="row"><td role="cell"><a href="/professions/alchemy/2-recipe/">Recipe</a></td><td role="cell"><span class="o">1</span></td><td role="cell">Second</td><td role="cell"></td></tr>"#.to_vec(),
                headers: vec![],
            });
        }
        self.0.request(request)
    }
}

#[test]
fn refresh_reports_sync_failure_without_stale_notice_or_stderr() -> Result<()> {
    for cached in [false, true] {
        for json in [false, true] {
            let world = World::new("36");
            if cached {
                assert!(world.sync()?.view().complete);
                world.advance(86401);
            }
            let next = world.http.requests.borrow().len() + 1;
            world
                .http
                .scripts
                .borrow_mut()
                .insert(next, Injected::Failure);
            let http = RefreshHttp(&world.http);
            let context = Context {
                http: &http,
                ..world.context()
            };
            let command = everledger::cli::Command::Refresh(everledger::commands::refresh::Args {
                target: everledger::commands::refresh::Target::Catalogue,
                json,
            });
            let mut stdout = Vec::new();
            assert_eq!(
                everledger::cli::execute(&command, &context, &mut stdout),
                everledger::ExitCode::Unreachable
            );
            assert!(world.diagnostics.contents().is_empty());
            let output = String::from_utf8(stdout).unwrap();
            if json {
                let doc = parse(&output)?;
                assert_eq!(doc.get("refreshed"), Some(&"catalogue".into()));
                let synced = doc.get("vendor_sync").unwrap();
                assert_eq!(synced.get("requests"), Some(&1i64.into()));
                assert_eq!(synced.get("failed"), Some(&true.into()));
                assert_eq!(
                    doc.get("sources").unwrap().as_array().unwrap().len(),
                    if cached { 2 } else { 1 }
                );
            } else {
                assert!(output.starts_with("catalogue refreshed\nForeverDB sync: failed; 1 requests this refresh; ForeverDB sync failed (HTTP 503)\n"));
                assert!(!output.contains("stale:"));
            }
        }
    }
    Ok(())
}

#[test]
fn refresh_stale_decision_uses_vendor_sources_after_successful_partial_sync() -> Result<()> {
    let mut world = World::new("36");
    assert!(world.sync()?.view().complete);
    world.advance(86401);
    world.cap("1");
    let http = RefreshHttp(&world.http);
    let context = Context {
        http: &http,
        ..world.context()
    };
    let args = everledger::commands::refresh::Args {
        target: everledger::commands::refresh::Target::Catalogue,
        json: false,
    };
    let output = everledger::commands::refresh::run(&args, &context)?;
    assert_eq!(output.code, everledger::ExitCode::Stale);
    assert_eq!(
        output.lines,
        [
            "catalogue refreshed",
            "ForeverDB sync: pending; 1 requests this refresh"
        ]
    );
    Ok(())
}
