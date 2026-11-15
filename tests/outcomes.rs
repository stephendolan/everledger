use everledger::{
    Context, ExitCode, Failure, Result,
    catalogue::CatalogueStore,
    cli,
    clock::VirtualClock,
    config::Config,
    diagnostics::WriterDiagnostics,
    document::{self, Document},
    http::{Http, Request, Response},
    time,
};
use std::{cell::RefCell, collections::VecDeque};

struct Fixture {
    responses: RefCell<VecDeque<Result<Response>>>,
    requests: RefCell<Vec<String>>,
}
impl Fixture {
    fn new(responses: impl IntoIterator<Item = Result<Response>>) -> Self {
        Self {
            responses: RefCell::new(responses.into_iter().collect()),
            requests: RefCell::default(),
        }
    }
}
impl Http for Fixture {
    fn request(&self, request: &Request) -> Result<Response> {
        self.requests.borrow_mut().push(request.url.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .expect("fixture response")
    }
}
fn body(text: &str) -> Result<Response> {
    Ok(Response {
        body: text.as_bytes().to_vec(),
        headers: vec![],
    })
}
fn conflict(code: &str) -> Result<Response> {
    let mut failure = Failure::error("fixture conflict");
    failure.body = Some(Document::object([
        ("status", Document::from(409_i64)),
        (
            "response",
            Document::object([("error_code", Document::from(code))]),
        ),
    ]));
    Err(failure)
}
fn execute(args: &[&str], http: &Fixture) -> (ExitCode, String, String) {
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
    let mut config = Config::from_values(&Default::default());
    config.roster_url = "http://127.0.0.1".into();
    config.ahledger_url = "http://127.0.0.1".into();
    config.roster_token = "fixture-token".into();
    let catalogue = CatalogueStore::from_document(
        document::parse(r#"{"generated_at":"2026-11-15T12:00:00Z","professions":{}}"#).unwrap(),
    )
    .unwrap();
    let diagnostics = WriterDiagnostics::default();
    let context = Context {
        clock: &clock,
        http,
        config: &config,
        catalogue: &catalogue,
        diagnostics: &diagnostics,
    };
    let command =
        cli::parse_from(std::iter::once("everledger").chain(args.iter().copied())).unwrap();
    let mut output = vec![];
    let code = cli::execute(&command, &context, &mut output);
    (
        code,
        String::from_utf8(output).unwrap(),
        String::from_utf8(diagnostics.contents()).unwrap(),
    )
}
#[test]
fn aggregate_snapshot_restart_discards_rows_and_sources() {
    let http = Fixture::new([
        body(
            r#"{"events":[{"id":"discard","type":"sale","venue":"auction","count":1,"proceeds_copper":99}],"has_more":true,"next_cursor":"page","sources":[{"file":"discard.lua"}]}"#,
        ),
        conflict("snapshot_changed"),
        body(
            r#"{"entries":[{"id":"keep","type":"sale","venue":"auction","count":2,"proceeds_copper":180}],"has_more":false,"checkpoint":"watch","sources":[{"file":"keep.lua"}]}"#,
        ),
    ]);
    let (code, output, stderr) = execute(&["sales", "--json"], &http);
    assert_eq!(code, ExitCode::Answer);
    let result = document::parse(&output).unwrap();
    assert_eq!(
        result.get("proceeds_copper").and_then(Document::as_i64),
        Some(180)
    );
    assert_eq!(
        result.get("after_cursor").and_then(Document::as_str),
        Some("watch")
    );
    assert!(output.contains("keep.lua"));
    assert!(!output.contains("discard"));
    assert_eq!(
        stderr,
        "snapshot_changed: restarting the complete query once\n"
    );
    assert_eq!(
        *http.requests.borrow(),
        [
            "http://127.0.0.1/events?limit=500&since=1794139200",
            "http://127.0.0.1/events?limit=500&cursor=page",
            "http://127.0.0.1/events?limit=500&since=1794139200",
        ]
    );
}
#[test]
fn aggregate_restarts_only_once() {
    let http = Fixture::new([conflict("snapshot_changed"), conflict("snapshot_changed")]);
    let (code, output, stderr) = execute(&["purchases", "--json"], &http);
    assert_eq!(code, ExitCode::Error);
    assert!(output.is_empty());
    assert_eq!(
        stderr.matches("restarting the complete query once").count(),
        1
    );
    assert_eq!(http.requests.borrow().len(), 2);
}
#[test]
fn inventory_explicit_null_availability_stays_unknown() {
    let http = Fixture::new([
        body("market|version|1794744000|2\n2604:88:87:709:::::3808\n"),
        body(r#"{"characters":[{"name":"Slade"}]}"#),
        body(r#"{"events":[],"has_more":false,"checkpoint":"watch"}"#),
        body(r#"{"listings":[]}"#),
        body(r#"{"bags":{"available":null,"items":[]}}"#),
    ]);
    let (code, output, stderr) = execute(&["positions", "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let result = document::parse(&output).unwrap();
    let locations = result
        .get("inventory_availability")
        .and_then(Document::as_array)
        .unwrap();
    assert!(matches!(
        locations[0].get("available"),
        Some(Document::Null)
    ));
    assert!(matches!(
        locations[1].get("available"),
        Some(Document::Bool(false))
    ));
    assert!(matches!(
        result.get("complete"),
        Some(Document::Bool(false))
    ));
}
#[test]
fn page_cursor_keeps_item_document_order_and_metadata() {
    // These cursor bytes are Python json.dumps(separators=(',', ':')) + urlsafe_b64encode.
    let cursor = "elpage1:eyJjdXJzb3IiOiJwYWdlLTEiLCJ2aWV3Ijp7Iml0ZW0iOnsibmFtZSI6IlJlZCBEeWUiLCJpZCI6MjYwNCwibWFya2VyIjoiXHUwMGU5In0sInR5cGUiOiJzYWxlIn0sInVudGlsIjoiMjA5OS0wMS0wMVQwMDowMDowMFoifQ==";
    let expected = "elpage1:eyJjdXJzb3IiOiJwYWdlLTIiLCJ2aWV3Ijp7Iml0ZW0iOnsibmFtZSI6IlJlZCBEeWUiLCJpZCI6MjYwNCwibWFya2VyIjoiXHUwMGU5In0sInR5cGUiOiJzYWxlIn0sInVudGlsIjoiMjA5OS0wMS0wMVQwMDowMDowMFoifQ==";
    let http = Fixture::new([body(
        r#"{"entries":[{"type":"sale","item":{"id":2604}},{"type":"purchase","item":{"id":2604}}],"has_more":true,"next_cursor":"page-2"}"#,
    )]);
    let (code, output, stderr) = execute(&["events", "--cursor", cursor, "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let result = document::parse(&output).unwrap();
    assert_eq!(
        result.get("next_cursor").and_then(Document::as_str),
        Some(expected)
    );
    assert_eq!(
        result
            .get("entries")
            .and_then(Document::as_array)
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(result.get("after_cursor"), Some(Document::Null)));
    assert_eq!(
        *http.requests.borrow(),
        ["http://127.0.0.1/events?cursor=page-1"]
    );
}

#[test]
fn numeric_positions_keep_nameless_inventory_and_linkless_listings() {
    let http = Fixture::new([
        body("market|version|1794744000|2\n2604:88:87:709:::::3808\n"),
        body(r#"{"characters":[{"name":"Slade"}]}"#),
        body(r#"{"events":[],"has_more":false,"checkpoint":"watch"}"#),
        body(r#"{"listings":[{"item":{"id":2604,"name":null},"count":2}]}"#),
        body(r#"{"bags":{"available":true,"items":[{"item_id":2604,"count":4}]}}"#),
    ]);
    let (code, output, stderr) = execute(&["positions", "--item", "2604", "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let result = document::parse(&output).unwrap();
    let rows = result
        .get("positions")
        .and_then(Document::as_array)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("held").and_then(Document::as_i64), Some(4));
    assert_eq!(
        rows[0].get("value_copper").and_then(Document::as_i64),
        Some(348)
    );
}

#[test]
fn linkless_listing_position_preserves_item_and_purchase_basis() {
    let http = Fixture::new([
        body("market|version|1794744000|2\n2604:88:87:709:::::3808\n"),
        body(r#"{"characters":[]}"#),
        body(
            r#"{"events":[{"type":"purchase","item":{"id":null,"name":"rEd DyE"},"count":2}],"has_more":false,"checkpoint":"watch"}"#,
        ),
        body(r#"{"listings":[{"item":{"name":"Red Dye","id":null,"id_basis":"name"},"count":2}]}"#),
    ]);
    let (code, output, stderr) = execute(&["positions", "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let result = document::parse(&output).unwrap();
    let rows = result
        .get("positions")
        .and_then(Document::as_array)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0].get("floor_copper"), Some(Document::Null)));
    assert_eq!(
        rows[0]
            .get("purchases")
            .and_then(Document::as_array)
            .unwrap()
            .len(),
        1
    );
    assert!(output.contains(r#""item": {"name": "Red Dye", "id": null, "id_basis": "name"}"#));
    assert!(output.contains(r#""match_basis": ["name"]"#));
}

#[test]
fn nested_listing_availability_null_is_preserved() {
    let http = Fixture::new([
        body("market|version|1794744000|2\n2604:88:87:709:::::3808\n"),
        body(
            r#"{"listings":[{"item_id":2604,"count":1,"buyout_copper":null,"price":{"available":null,"recorded":{"buyout_copper":200}}}]}"#,
        ),
    ]);
    let (code, output, stderr) = execute(&["listings", "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let result = document::parse(&output).unwrap();
    let rows = result.get("listings").and_then(Document::as_array).unwrap();
    assert!(matches!(rows[0].get("available"), Some(Document::Null)));
    assert!(matches!(rows[0].get("buyout_copper"), Some(Document::Null)));
    assert!(matches!(rows[0].get("undercut"), Some(Document::Null)));
}

#[test]
fn character_resolution_precedes_after_validation_and_allows_null_name() {
    let http = Fixture::new([
        body(r#"{"character":{"name":null}}"#),
        body(r#"{"events":[],"has_more":false,"checkpoint":"watch"}"#),
    ]);
    let (code, _, stderr) = execute(
        &["events", "--char", "Slade", "--after", "old", "--json"],
        &http,
    );
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    assert_eq!(
        *http.requests.borrow(),
        [
            "http://127.0.0.1/characters/Slade",
            "http://127.0.0.1/events?limit=500&after=old"
        ]
    );
    let http = Fixture::new([body(r#"{"character":{"name":"Slade"}}"#)]);
    let (code, _, stderr) = execute(
        &["events", "--char", "Slade", "--after", "", "--json"],
        &http,
    );
    assert_eq!(code, ExitCode::Usage);
    assert_eq!(
        *http.requests.borrow(),
        ["http://127.0.0.1/characters/Slade"]
    );
    assert_eq!(
        stderr,
        "empty --after; bootstrap with: everledger events --limit 0 --json\n"
    );
}
