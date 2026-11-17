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
}
impl Clock for TestClock {
    fn now(&self) -> UtcInstant {
        self.wall.get()
    }
    fn monotonic(&self) -> f64 {
        self.elapsed.get()
    }
    fn sleep(&self, seconds: f64) -> Result<()> {
        self.elapsed.set(self.elapsed.get() + seconds);
        Ok(())
    }
}
enum Injected {
    Retryable,
    Failure,
    AbruptExit,
}
struct Fixture {
    clock: Rc<TestClock>,
    data: RefCell<Document>,
    requests: RefCell<Vec<(String, f64)>>,
    scripts: RefCell<HashMap<usize, Injected>>,
    advance_after_request: Cell<i64>,
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
        });
        let http = Fixture {
            clock: Rc::clone(&clock),
            data: RefCell::new(parse(include_str!("fixtures/foreverdb.json")).unwrap()),
            requests: RefCell::new(vec![]),
            scripts: RefCell::new(HashMap::new()),
            advance_after_request: Cell::new(0),
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
            r#"{"configured":true,"complete":true,"pending":false,"requests":0,"deferred":"daily sync limit","failed":false}"#
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
            r#"{"configured":true,"complete":true,"pending":false,"requests":4,"total_requests":6}"#
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
fn legacy_checkpoint_migrates_own_requests_at_finish_or_last_request() -> Result<()> {
    for timestamp in ["finished_at", "last_request_at"] {
        let mut world = World::new("2");
        let mut legacy = parse(
            r#"{"attempted_at":"2026-11-13T12:00:00Z","requests":2,"total_requests":100,"sync_id":"legacy","failed":false}"#,
        )?;
        legacy.set(timestamp, "2026-11-15T11:26:00Z")?;
        world.write("attempt.json", &legacy);
        let before = std::fs::read(world.path("attempt.json"))?;
        assert_eq!(world.sync()?.view().requests, 0);
        assert!(world.http.requests.borrow().is_empty());
        assert_eq!(std::fs::read(world.path("attempt.json"))?, before);
        world.cap("6");
        let resumed = world.sync()?;
        assert_eq!(resumed.view().requests, 4);
        assert!(!resumed.view().complete);
        assert_eq!(
            world.spends().as_array().unwrap()[0],
            spend(2, "2026-11-15T11:26:00Z")
        );
        assert_eq!(world.sync()?.view().requests, 0);
    }
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
    assert!(world.sync().is_err());
    assert!(foreverdb::index(&world.context())?.is_none());
    assert!(!world.path("pending.json").exists());
    assert!(world.sync().is_err());
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
    let failure = world.sync().unwrap_err();
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
fn attempt_reader_migrates_and_prunes_without_writing() -> Result<()> {
    for finished in [None, Some(""), Some("2026-11-15T11:00:00Z")] {
        let world = World::new("36");
        let mut legacy = parse(
            r#"{"attempted_at":"2026-11-15T10:00:00Z","last_request_at":"2026-11-15T11:30:00Z"}"#,
        )?;
        if let Some(finished) = finished {
            legacy.set("finished_at", finished)?;
        }
        world.write("attempt.json", &legacy);
        let before = std::fs::read(world.path("attempt.json"))?;
        let read = foreverdb::read(&world.context(), "attempt.json")?.unwrap();
        let expected = finished
            .filter(|s| !s.is_empty())
            .unwrap_or("2026-11-15T11:30:00Z");
        assert_eq!(
            read.get("spends"),
            Some(&Document::Array(vec![spend(0, expected)]))
        );
        assert_eq!(foreverdb::attempt(&world.context())?.unwrap().requests, 0);
        assert_eq!(std::fs::read(world.path("attempt.json"))?, before);
    }
    let world = World::new("36");
    let legacy = parse(r#"{"attempted_at":"2026-11-15T11:00:00Z"}"#)?;
    world.write("attempt.json", &legacy);
    let migrated = foreverdb::read(&world.context(), "attempt.json")?.unwrap();
    assert_eq!(
        migrated.get("spends"),
        Some(&Document::Array(vec![spend(0, "2026-11-15T11:00:00Z")]))
    );
    let mut record = parse(
        r#"{"attempted_at":"2026-11-15T10:00:00Z","finished_at":"2026-11-15T11:00:00Z","spends":[]}"#,
    )?;
    let mut extra = spend(1, "2026-11-15T12:00:01Z");
    extra.set("unknown", "retained")?;
    record.set(
        "spends",
        Document::Array(vec![
            spend(1, "2026-11-14T12:00:00Z"),
            spend(1, "2026-11-14T12:00:00.000001Z"),
            extra.clone(),
        ]),
    )?;
    world.write("attempt.json", &record);
    let before = std::fs::read(world.path("attempt.json"))?;
    let read = foreverdb::read(&world.context(), "attempt.json")?.unwrap();
    assert_eq!(
        read.get("spends"),
        Some(&Document::Array(vec![
            spend(1, "2026-11-14T12:00:00.000001Z"),
            extra
        ]))
    );
    assert_eq!(std::fs::read(world.path("attempt.json"))?, before);
    Ok(())
}

#[test]
fn spend_fields_are_required_at_the_journal_boundary() -> Result<()> {
    let world = World::new("36");
    for bad in [
        r#"{"finished_at":"2026-11-15T12:00:00Z"}"#,
        r#"{"requests":1}"#,
    ] {
        let mut record = parse(
            r#"{"attempted_at":"2026-11-15T12:00:00Z","finished_at":"2026-11-15T12:00:00Z","spends":[]}"#,
        )?;
        record.set("spends", Document::Array(vec![parse(bad)?]))?;
        world.write("attempt.json", &record);
        assert!(foreverdb::read(&world.context(), "attempt.json").is_err());
        assert!(world.sync().is_err());
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
