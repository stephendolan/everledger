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
    requests: RefCell<Vec<(String, u32, u64)>>,
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
        assert_eq!(*attempt, i as u32 + 1);
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
    let dir = Directory::new();
    let config = dir.config();
    let http = Fixture {
        config: &config,
        total: 40,
        requests: RefCell::new(vec![]),
        fail_at: Some(36),
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
    assert_eq!(foreverdb::sync(&context)?.view().requests, 36);
    let pending = foreverdb::read(&context, "pending.json")?.expect("checkpoint");
    assert!(pending.get("retry").is_some());
    let tomorrow = VirtualClock::new(instant("2026-11-16T13:00:00.789012Z")?);
    context.clock = &tomorrow;
    assert_eq!(foreverdb::sync(&context)?.view().requests, 6);
    assert!(foreverdb::read(&context, "pending.json")?.is_none());
    assert!(foreverdb::index(&context)?.is_some());
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
