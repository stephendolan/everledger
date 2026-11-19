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
    let mut config = Config::from_values(&Default::default()).unwrap();
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
            r#"{"has_more":true,"next_cursor":"page","sources":[{"file":"discard.lua"}],"entries":[{"id":"discard","type":"sale","venue":"auction","count":1,"proceeds_copper":99}]}"#,
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
        body(r#"{"has_more":false,"checkpoint":"watch","entries":[]}"#),
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
        body(r#"{"has_more":false,"checkpoint":"watch","entries":[]}"#),
        body(r#"{"listings":[{"count":2,"item_id":2604,"name":null}]}"#),
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
            r#"{"has_more":false,"checkpoint":"watch","entries":[{"type":"purchase","item":{"id":null,"name":"rEd DyE"},"count":2}]}"#,
        ),
        body(r#"{"listings":[{"count":2,"item_id":null,"name":"Red Dye"}]}"#),
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
    assert!(output.contains(r#""item": {"id": null, "name": "Red Dye"}"#));
    assert!(output.contains(r#""match_basis": ["name"]"#));
}

#[test]
fn nested_listing_availability_null_is_preserved() {
    let http = Fixture::new([
        body("market|version|1794744000|2\n2604:88:87:709:::::3808\n"),
        body(
            r#"{"listings":[{"item_id":2604,"count":1,"price":{"available":null,"recorded":{"buyout_copper":null}}}]}"#,
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
        body(r#"{"has_more":false,"checkpoint":"watch","entries":[]}"#),
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

#[test]
fn stale_notice_and_exit_code_follow_the_same_freshness_decision() {
    for json in [false, true] {
        for (command, stale) in [
            ("sales", false),
            ("purchases", false),
            ("events", false),
            ("expirations", true),
            ("net", true),
        ] {
            let journal = body(
                r#"{"sources":[{"file":"Journalator.lua","mtime":"2020-01-01T00:00:00Z","stale":true}],"entries":[{"type":"posting","venue":"auction","item":{"id":2604},"count":1},{"type":"expiration","venue":"auction","item":{"id":2604},"count":1}]}"#,
            );
            let mut responses = vec![journal];
            if command == "expirations" {
                responses.push(body("m|v|1\n2604:88:87:709:::::10"));
            } else if command == "net" {
                responses.push(body(r#"{"listings":[],"scans":[{"available":true,"pricing":{"available":true,"source":{"file":"scan.lua","stale":true}}}]}"#));
            }
            let http = Fixture::new(responses);
            let mut args = vec![command];
            if command == "net" {
                args.push("2604");
            }
            if json {
                args.push("--json");
            }
            let (code, output, stderr) = execute(&args, &http);
            assert_eq!(
                code,
                if stale {
                    ExitCode::Stale
                } else {
                    ExitCode::Answer
                },
                "{args:?}: {stderr}"
            );
            assert!(stderr.is_empty());
            if json {
                let data = document::parse(&output).unwrap();
                if command == "net" {
                    assert_eq!(data.get("stale"), Some(&Document::Bool(true)));
                }
            } else {
                assert_eq!(
                    output
                        .matches("stale: data exceeds its freshness limit")
                        .count(),
                    usize::from(stale),
                    "{args:?}"
                );
                let lines = output.lines().collect::<Vec<_>>();
                assert!(lines.last().unwrap().starts_with("source:"));
                assert!(lines.last().unwrap().contains("Journalator.lua"));
                if stale {
                    assert_eq!(
                        lines[lines.len() - 2],
                        "stale: data exceeds its freshness limit"
                    );
                }
            }
        }
    }
}

#[test]
fn expirations_without_rows_never_fetch_the_market_and_old_journal_is_not_stale() {
    for json in [false, true] {
        for rows in [false, true] {
            let mut responses = vec![body(if rows {
                r#"{"sources":[{"file":"old.lua","stale":true}],"entries":[{"type":"expiration","item":{"id":2604},"count":1}]}"#
            } else {
                r#"{"sources":[{"file":"old.lua","stale":true}],"entries":[]}"#
            })];
            if rows {
                responses.push(body("m|v|1794744000\n2604:88:87:709.9:::::10"));
            }
            let http = Fixture::new(responses);
            let mut args = vec!["expirations"];
            if json {
                args.push("--json");
            }
            let (code, output, stderr) = execute(&args, &http);
            assert_eq!(code, ExitCode::Answer, "{stderr}");
            assert!(!output.contains("stale: data exceeds"));
            assert_eq!(http.requests.borrow().len(), if rows { 2 } else { 1 });
            if rows && json {
                let json: serde_json::Value = serde_json::from_str(&output).unwrap();
                assert!(json["expirations"][0]["market"]["quantity"].is_i64());
                let data = document::parse(&output).unwrap();
                assert_eq!(
                    data.get("expirations").unwrap().as_array().unwrap()[0]
                        .get("market")
                        .unwrap()
                        .get("quantity"),
                    Some(&Document::from(709_i64))
                );
            } else if rows {
                assert!(output.contains("listed 709 |"));
            }
        }
    }
}

#[test]
fn listing_sources_keep_only_first_occurrences_in_order() {
    let http = Fixture::new([
        body("m|v|1794744000\n2604:88:87:709:::::10"),
        body(
            r#"{"listings":[],"sources":[{"file":"A","stale":false},{"stale":false,"file":"A"},{"file":"B","stale":false}],"scans":[{"pricing":{"source":{"file":"A","stale":false}}},{"pricing":{"source":{"file":"C","stale":false}}},{"pricing":{"source":{"file":"C","stale":false}}}]}"#,
        ),
    ]);
    let (code, output, stderr) = execute(&["listings", "--json"], &http);
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    let data = document::parse(&output).unwrap();
    let used = data.get("sources").unwrap().as_array().unwrap();
    assert_eq!(used.len(), 4);
    assert_eq!(
        used.iter()
            .take(3)
            .map(|s| s.get("file").unwrap().as_str().unwrap())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert_eq!(
        document::dumps(&used[0], document::JsonMode::Stdout),
        r#"{"file": "A", "stale": false}"#
    );
    assert_eq!(data.get("scans").unwrap().as_array().unwrap().len(), 3);
}

#[test]
fn selected_positions_resolve_the_character_once_and_use_the_resolved_name() {
    let http = Fixture::new([
        body("m|v|1794744000\n2604:88:87:709:::::10"),
        body(r#"{"characters":[{"name":"Other"}]}"#),
        body(r#"{"character":{"name":"Slade Undertow","short_name":"Wrong"}}"#),
        body(r#"{"entries":[]}"#),
        body(r#"{"listings":[]}"#),
        body(r#"{"bags":{"available":true,"items":[{"item_id":2604,"count":4}]}}"#),
    ]);
    let (code, output, stderr) = execute(
        &["positions", "--char", "Slade", "--item", "2604", "--json"],
        &http,
    );
    assert_eq!(code, ExitCode::Answer, "{stderr}");
    assert_eq!(
        *http.requests.borrow(),
        [
            "http://127.0.0.1/v1/pricetable/forever.pvp.alliance.us",
            "http://127.0.0.1/characters",
            "http://127.0.0.1/characters/Slade",
            "http://127.0.0.1/events?limit=500&since=1794139200",
            "http://127.0.0.1/listings?char=Slade&item=2604",
            "http://127.0.0.1/characters/Slade%20Undertow/inventory",
        ]
    );
    let data = document::parse(&output).unwrap();
    let row = &data.get("positions").unwrap().as_array().unwrap()[0];
    assert_eq!(row.get("held").and_then(Document::as_i64), Some(4));
    assert_eq!(
        row.get("locations").unwrap().as_array().unwrap()[0]
            .get("character")
            .and_then(Document::as_str),
        Some("Slade Undertow")
    );
}

#[test]
fn reconciled_net_omits_deposits_lost_and_carries_freshness() {
    for json in [false, true] {
        let http = Fixture::new([
            body(
                r#"{"sources":[{"file":"old.lua","stale":true}],"entries":[{"type":"posting","venue":"auction","item":{"id":2604},"count":1},{"type":"sale","venue":"auction","item":{"id":2604},"count":1}]}"#,
            ),
            body(r#"{"listings":[],"sources":[{"file":"scan.lua","stale":false}]}"#),
        ]);
        let mut args = vec!["net", "2604"];
        if json {
            args.push("--json");
        }
        let (code, output, stderr) = execute(&args, &http);
        assert_eq!(code, ExitCode::Answer, "{stderr}");
        assert!(!output.contains("deposits lost"));
        assert!(!output.contains("stale: data exceeds"));
        if json {
            let data = document::parse(&output).unwrap();
            assert_eq!(data.get("unaccounted"), Some(&Document::from(0_i64)));
            assert_eq!(data.get("deposits_lost_copper"), Some(&Document::Null));
            assert_eq!(data.get("stale"), Some(&Document::Bool(false)));
        }
    }
}

#[test]
fn positions_freshness_uses_market_roster_inventory_and_listings_but_not_journal_age() {
    for stale_kind in [
        "none",
        "market",
        "characters",
        "listing",
        "inventory",
        "journal",
    ] {
        for json in [false, true] {
            let stale = ["market", "characters", "listing", "inventory"].contains(&stale_kind);
            let epoch = if stale_kind == "market" {
                1
            } else {
                1794744000
            };
            let http = Fixture::new([
                body(&format!("m|v|{epoch}\n2604:88:87:709:::::10")),
                body(&format!(
                    r#"{{"characters":[{{"name":"Slade"}}],"sources":[{{"file":"roster.lua","stale":{}}}]}}"#,
                    stale_kind == "characters"
                )),
                body(&format!(
                    r#"{{"entries":[],"sources":[{{"file":"Journalator.lua","mtime":"2020-01-01T00:00:00Z","stale":{}}}]}}"#,
                    stale_kind == "journal"
                )),
                body(&format!(
                    r#"{{"listings":[],"scans":[{{"pricing":{{"source":{{"file":"scan.lua","stale":{}}}}}}}]}}"#,
                    stale_kind == "listing"
                )),
                body(&format!(
                    r#"{{"sources":[{{"file":"inventory.lua","stale":{}}}]}}"#,
                    stale_kind == "inventory"
                )),
            ]);
            let mut args = vec!["positions"];
            if json {
                args.push("--json");
            }
            let (code, output, stderr) = execute(&args, &http);
            assert_eq!(
                code,
                if stale {
                    ExitCode::Stale
                } else {
                    ExitCode::Answer
                },
                "{stale_kind}: {stderr}"
            );
            assert_eq!(
                output.matches("stale: data exceeds").count(),
                usize::from(stale && !json)
            );
            if json {
                assert_eq!(
                    document::parse(&output).unwrap().get("stale"),
                    Some(&Document::Bool(stale))
                );
            }
        }
    }
}
