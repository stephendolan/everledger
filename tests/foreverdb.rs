use everledger::{
    Context, Result,
    clock::VirtualClock,
    config::Config,
    diagnostics::WriterDiagnostics,
    document::{Document, parse},
    foreverdb,
    http::{Http, Request, Response},
    time::instant,
};
use std::{cell::RefCell, collections::HashMap, path::PathBuf};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "everledger-sync-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir(&path).expect("directory");
        Self(path)
    }
    fn config(&self) -> Config {
        Config::from_values(&HashMap::from([
            (
                "EVERLEDGER_DATA_DIR".into(),
                self.0.to_string_lossy().into_owned(),
            ),
            ("EVERLEDGER_FOREVERDB_API_KEY".into(), "fixture".into()),
            (
                "EVERLEDGER_SEAM_FOREVERDB_URL".into(),
                "http://127.0.0.1:1".into(),
            ),
        ]))
        .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture<'a> {
    config: &'a Config,
    total: usize,
    requests: RefCell<Vec<(String, u64, u64)>>,
    fail_at: Option<usize>,
}
impl Http for Fixture<'_> {
    fn request(&self, request: &Request) -> Result<Response> {
        let attempt = parse(&std::fs::read_to_string(
            self.config
                .data_dir
                .join("vendors/foreverdb.net/attempt.json"),
        )?)?;
        let state: foreverdb::Attempt = attempt.decode("attempt")?;
        let mut requests = self.requests.borrow_mut();
        requests.push((request.url.clone(), state.requests, state.total_requests));
        if Some(requests.len()) == self.fail_at {
            return Err(everledger::Failure {
                message: "secret upstream error".into(),
                code: everledger::ExitCode::Unreachable,
                body: Some(Document::object([
                    ("status", 502i64.into()),
                    (
                        "response",
                        Document::object([
                            ("retryable", true.into()),
                            ("retry_after", 60i64.into()),
                        ]),
                    ),
                ])),
            });
        }
        let body = if request.url.contains('?') {
            format!(
                "{{\"data\":[{}],\"meta\":{{\"build\":\"fixture\",\"total\":{},\"limit\":100,\"offset\":0}}}}",
                (1..=self.total)
                    .map(|id| format!("{{\"id\":{id}}}"))
                    .collect::<Vec<_>>()
                    .join(","),
                self.total
            )
        } else {
            let id = request.url.rsplit('/').next().unwrap_or("0");
            format!(
                "{{\"data\":{{\"id\":{id},\"name\":\"Seller\",\"zone\":\"Ironforge\",\"sells\":[{{\"itemId\":2604,\"name\":\"Red Dye\",\"price\":10,\"stack\":3,\"limited\":true,\"seenInForever\":true,\"pricePerItem\":3.3333333333333335}}]}}}}"
            )
        };
        Ok(Response {
            body: body.into_bytes(),
            headers: vec![],
        })
    }
}
#[test]
fn journal_precedes_transport_and_budgeted_traversal_resumes() -> Result<()> {
    let dir = Directory::new();
    let config = dir.config();
    let http = Fixture {
        config: &config,
        total: 40,
        requests: RefCell::new(vec![]),
        fail_at: None,
    };
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let diag = WriterDiagnostics::<Vec<u8>>::default();
    let clock = VirtualClock::new(instant("2026-11-15T12:00:00Z")?);
    let mut context = Context {
        config: &config,
        http: &http,
        clock: &clock,
        catalogue: &catalogue,
        diagnostics: &diag,
    };
    let result = foreverdb::sync(&context)?;
    assert!(!result.view().complete);
    assert_eq!(result.view().requests, 36);
    assert!(foreverdb::index(&context)?.is_none());
    let requests = http.requests.borrow();
    for (i, (_, attempt, total)) in requests.iter().enumerate() {
        assert_eq!(*attempt, i as u64 + 1);
        assert_eq!(*total, i as u64 + 1);
    }
    drop(requests);
    assert_eq!(foreverdb::sync(&context)?.view().requests, 0);
    let tomorrow = VirtualClock::new(instant("2026-11-16T13:00:00Z")?);
    context.clock = &tomorrow;
    let resumed = foreverdb::sync(&context)?;
    assert!(resumed.view().complete);
    assert_eq!(resumed.view().requests, 5);
    let idx = foreverdb::index(&context)?.expect("complete index");
    let answer = foreverdb::lookup(2604, &idx, &context)?;
    let rows = answer
        .get("vendors")
        .and_then(Document::as_array)
        .expect("vendors");
    assert_eq!(rows.len(), 40);
    assert_eq!(rows[0].get("price_copper"), Some(&Document::Null));
    assert_eq!(
        rows[0].get("stack_price_copper"),
        Some(&Document::from(10i64))
    );
    assert_eq!(
        rows[0]
            .get("raw")
            .and_then(|d| d.get("sell"))
            .and_then(|d| d.get("pricePerItem")),
        Some(&Document::from(3.3333333333333335))
    );
    Ok(())
}
#[test]
fn retry_at_cap_stays_pending_and_resumes_once() -> Result<()> {
    for (cap, resumed_requests, complete) in [
        (1, 1, false),
        (2, 2, false),
        (36, 6, true),
        (40, 2, true),
        (41, 1, true),
    ] {
        let dir = Directory::new();
        let mut config = dir.config();
        config.foreverdb_daily_requests = Config::from_values(&HashMap::from([(
            "EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(),
            cap.to_string(),
        )]))?
        .foreverdb_daily_requests;
        let http = Fixture {
            config: &config,
            total: 40,
            requests: RefCell::new(vec![]),
            fail_at: Some(cap),
        };
        let catalogue = everledger::catalogue::CatalogueStore::default();
        let diag = WriterDiagnostics::<Vec<u8>>::default();
        let clock = VirtualClock::new(instant("2026-11-15T12:00:00.789012Z")?);
        let mut context = Context {
            config: &config,
            http: &http,
            clock: &clock,
            catalogue: &catalogue,
            diagnostics: &diag,
        };
        assert_eq!(foreverdb::sync(&context)?.view().requests, cap as u64);
        let pending = foreverdb::read(&context, "pending.json")?.expect("checkpoint");
        assert!(pending.get("retry").is_some());
        let tomorrow = VirtualClock::new(instant("2026-11-16T13:00:00.789012Z")?);
        context.clock = &tomorrow;
        let resumed = foreverdb::sync(&context)?;
        assert_eq!(resumed.view().requests, resumed_requests);
        assert_eq!(resumed.view().complete, complete);
        assert_eq!(
            foreverdb::read(&context, "pending.json")?.is_none(),
            complete
        );
        assert_eq!(foreverdb::index(&context)?.is_some(), complete);
        if let Some(pending) = foreverdb::read(&context, "pending.json")? {
            assert!(pending.get("retry").is_none());
        }
    }
    Ok(())
}
#[test]
fn mapping_rejects_wrong_shapes_and_retains_exact_stack_price() -> Result<()> {
    let fixture = parse(include_str!("fixtures/foreverdb.json"))?;
    let vendor = fixture
        .get("details")
        .and_then(|d| d.get("8681"))
        .expect("fixture vendor");
    let mapped = foreverdb::mapping::vendor(vendor.clone(), foreverdb::mapping::Identifier(8681))?;
    assert!(mapped.get("sells").and_then(Document::as_array).is_some());
    for raw in [
        "{}",
        r#"{"data":[],"meta":{}}"#,
        r#"{"data":[{"id":true}],"meta":{"total":1,"limit":100,"offset":0,"build":"x"}}"#,
    ] {
        let error = foreverdb::mapping::page(parse(raw)?, 0)
            .err()
            .expect("invalid page");
        assert_eq!(error.code, everledger::ExitCode::Unreachable);
        assert_eq!(
            error.message,
            "ForeverDB API shape mismatch; check vendor mapping before syncing"
        );
    }
    Ok(())
}

#[test]
fn unlinked_catalogue_results_follow_both_oracle_cleaning_passes() -> Result<()> {
    struct Pages;
    impl Http for Pages {
        fn request(&self, _: &Request) -> Result<Response> {
            let rows=["Rune &lt;Major&gt;", "Plain &amp;amp; Simple"].into_iter().enumerate().map(|(i,name)| format!(r#"<tr role="row"><td role="cell"><a href="/professions/alchemy/{}-recipe/">Recipe</a></td><td role="cell"><span class="o">1</span></td><td role="cell">{name}</td><td role="cell"></td></tr>"#,i+1)).collect::<String>();
            Ok(Response {
                body: format!("Recipes (2){rows}").into_bytes(),
                headers: vec![],
            })
        }
    }
    let dir = Directory::new();
    let config = dir.config();
    let clock = VirtualClock::new(instant("2026-11-15T12:00:00Z")?);
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let diagnostics = WriterDiagnostics::<Vec<u8>>::default();
    let context = Context {
        config: &config,
        clock: &clock,
        http: &Pages,
        catalogue: &catalogue,
        diagnostics: &diagnostics,
    };
    let built = everledger::catalogue_build::build(&context)?;
    let recipes = &built
        .view()
        .professions
        .get("alchemy")
        .expect("alchemy")
        .recipes;
    assert_eq!(recipes[0].result.name, "Rune");
    assert_eq!(recipes[1].result.name, "Plain & Simple");
    Ok(())
}

#[test]
fn foreverdb_ignores_comparison_caches_from_a_different_ninja_url() -> Result<()> {
    let dir = Directory::new();
    let config = dir.config();
    let http = Fixture {
        config: &config,
        total: 1,
        requests: RefCell::new(vec![]),
        fail_at: None,
    };
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let diagnostics = WriterDiagnostics::<Vec<u8>>::default();
    let clock = VirtualClock::new(instant("2026-11-15T12:00:00Z")?);
    let context = Context {
        config: &config,
        http: &http,
        clock: &clock,
        catalogue: &catalogue,
        diagnostics: &diagnostics,
    };
    assert!(foreverdb::sync(&context)?.view().complete);
    let path = config.data_dir.join("vendors/wowforever.ninja");
    std::fs::create_dir_all(&path)?;
    std::fs::write(
        path.join("2604.json"),
        r#"{"backend_url":"http://127.0.0.1:9999","vendors":"unrelated shape","published_at":false}"#,
    )?;
    assert!(matches!(
        everledger::vendors::lookup(2604, false, &context)?,
        everledger::vendors::VendorAnswer::Foreverdb(_)
    ));
    let (status, _) = everledger::vendors::status::backend_status(&context)?;
    assert_eq!(status.view().coverage.ninja.item_count, 0);
    assert_eq!(http.requests.borrow().len(), 2);
    Ok(())
}

#[test]
fn configured_caps_bound_requests_without_changing_pacing_or_publication() -> Result<()> {
    struct TimedHttp<'a> {
        fixture: Fixture<'a>,
        clock: &'a VirtualClock,
        starts: RefCell<Vec<f64>>,
    }
    impl Http for TimedHttp<'_> {
        fn request(&self, request: &Request) -> Result<Response> {
            use everledger::clock::Clock;
            self.starts.borrow_mut().push(self.clock.monotonic());
            self.fixture.request(request)
        }
    }
    for (cap, expected_requests, complete) in [
        ("1", 1, false),
        ("40", 40, false),
        ("41", 41, true),
        ("5000", 41, true),
        ("4294967296", 41, true),
        ("9223372036854775807", 41, true),
    ] {
        let dir = Directory::new();
        let mut config = dir.config();
        config.foreverdb_daily_requests = Config::from_values(&HashMap::from([(
            "EVERLEDGER_FOREVERDB_DAILY_REQUESTS".into(),
            cap.into(),
        )]))?
        .foreverdb_daily_requests;
        let clock = VirtualClock::new(instant("2026-11-15T12:00:00.789012Z")?);
        let http = TimedHttp {
            fixture: Fixture {
                config: &config,
                total: 40,
                requests: RefCell::new(vec![]),
                fail_at: None,
            },
            clock: &clock,
            starts: RefCell::new(vec![]),
        };
        let catalogue = everledger::catalogue::CatalogueStore::default();
        let diagnostics = WriterDiagnostics::<Vec<u8>>::default();
        let context = Context {
            config: &config,
            http: &http,
            clock: &clock,
            catalogue: &catalogue,
            diagnostics: &diagnostics,
        };
        let result = foreverdb::sync(&context)?;
        assert_eq!(result.view().requests, expected_requests, "cap {cap}");
        assert_eq!(result.view().complete, complete, "cap {cap}");
        assert_eq!(foreverdb::index(&context)?.is_some(), complete, "cap {cap}");
        assert_eq!(
            foreverdb::read(&context, "pending.json")?.is_none(),
            complete,
            "cap {cap}"
        );
        let requests = http.fixture.requests.borrow();
        assert_eq!(requests.len() as u64, expected_requests, "cap {cap}");
        for (i, (_, attempt, total)) in requests.iter().enumerate() {
            assert_eq!(*attempt, i as u64 + 1, "cap {cap}");
            assert_eq!(*total, i as u64 + 1, "cap {cap}");
        }
        for pair in http.starts.borrow().windows(2) {
            assert!(
                (pair[1] - pair[0] - 2.1).abs() < 1e-10,
                "cap {cap}: {pair:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn index_is_read_with_or_without_details_and_refused_when_counts_disagree() -> Result<()> {
    let dir = Directory::new();
    let config = dir.config();
    let http = Fixture {
        config: &config,
        total: 3,
        requests: RefCell::new(vec![]),
        fail_at: None,
    };
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let diag = WriterDiagnostics::<Vec<u8>>::default();
    let clock = VirtualClock::new(instant("2026-11-15T12:00:00Z")?);
    let context = Context {
        config: &config,
        http: &http,
        clock: &clock,
        catalogue: &catalogue,
        diagnostics: &diag,
    };
    assert!(foreverdb::sync(&context)?.view().complete);
    let path = config.data_dir.join("vendors/foreverdb.net/index.json");
    let published: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path)?).expect("published index is JSON");
    let rewrite = |edit: &dyn Fn(&mut serde_json::Map<String, serde_json::Value>)| {
        let mut index = published.clone();
        edit(index.as_object_mut().expect("index is an object"));
        std::fs::write(&path, index.to_string()).expect("rewrite index");
    };
    let records = |context: &Context<'_>| -> Result<Option<usize>> {
        Ok(foreverdb::index(context)?.map(|index| index.view().record_count))
    };
    assert_eq!(records(&context)?, Some(3));
    // The reference never reads `details` or `sync_requests`: an index without them is whole.
    rewrite(&|index| {
        index.remove("details");
        index.remove("sync_requests");
    });
    assert_eq!(records(&context)?, Some(3));
    rewrite(&|index| {
        index.insert("details".into(), serde_json::Value::Null);
    });
    assert_eq!(records(&context)?, Some(3));
    // A block or a count that disagrees with the rest marks a damaged file.
    rewrite(&|index| {
        index.insert("details".into(), serde_json::json!({}));
    });
    assert_eq!(records(&context)?, None);
    rewrite(&|index| {
        index.insert("record_count".into(), 2.into());
    });
    assert_eq!(records(&context)?, None);
    rewrite(&|index| {
        index.insert("item_count".into(), 7.into());
    });
    assert_eq!(records(&context)?, None);
    Ok(())
}
