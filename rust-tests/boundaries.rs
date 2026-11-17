use everledger::{
    ExitCode,
    clock::{Clock, VirtualClock},
    document::{self, Document},
    http::{Http, Request, UreqHttp},
    time,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn cli_surface_is_consistent() {
    everledger::cli::parser().debug_assert();
}
#[test]
fn cli_rejects_unknowns_and_keeps_last_options() {
    for args in [
        vec!["--json", "verify"],
        vec!["price"],
        vec!["bridge", "tailoring", "1"],
        vec!["purchases", "--venue", "both"],
        vec!["events", "--lim", "1"],
        vec!["margins", "tailoring", "12"],
        vec!["positions", "--since", "7d"],
    ] {
        assert!(
            everledger::cli::parser()
                .try_get_matches_from(std::iter::once("everledger").chain(args))
                .is_err()
        );
    }
    let matches = everledger::cli::parser()
        .try_get_matches_from([
            "everledger",
            "events",
            "--limit",
            "20",
            "--limit",
            "-1",
            "--json",
            "--json",
        ])
        .unwrap();
    let (_, args) = matches.subcommand().unwrap();
    assert_eq!(args.get_one::<i64>("limit"), Some(&-1));
    assert!(args.get_flag("json"));
    let matches = everledger::cli::parser()
        .try_get_matches_from(["everledger", "char", "Slade", "tailoring"])
        .unwrap();
    assert_eq!(
        matches
            .subcommand()
            .unwrap()
            .1
            .get_one::<String>("profession")
            .unwrap(),
        "tailoring"
    );
}
#[test]
fn virtual_clock_uses_one_elapsed_value() {
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00.000001Z").unwrap());
    clock.sleep(0.0000005).unwrap();
    assert_eq!(clock.monotonic(), 0.0000005);
    assert_eq!(
        clock.now().microseconds(),
        time::instant("2026-11-15T12:00:00.000001Z")
            .unwrap()
            .microseconds()
    );
    clock.sleep(2.1).unwrap();
    assert_eq!(
        clock.now().microseconds(),
        time::instant("2026-11-15T12:00:02.100001Z")
            .unwrap()
            .microseconds()
    );
}
#[test]
fn items_respect_explicit_nulls_and_python_equality() {
    use everledger::catalogue::{Item, ItemRow, item_matches};
    let item = Item {
        id: 42,
        name: Some("STRASSE".into()),
    };
    for (payload, expected) in [
        (r#"{"item":{"id":null},"item_id":42}"#, false),
        (r#"{"item":{},"item_id":42}"#, true),
        (r#"{"item":{"name":"Straße"}}"#, true),
        (r#"{"item":{"name":null},"name":"Straße"}"#, false),
    ] {
        let row: ItemRow = document::parse(payload).unwrap().decode("test").unwrap();
        assert_eq!(item_matches(&row, &item), expected);
    }
    assert_eq!(
        document::parse("0").unwrap(),
        document::parse("-0.0").unwrap()
    );
    assert_ne!(
        document::parse("9007199254740993").unwrap(),
        document::parse("9007199254740992.0").unwrap()
    );
    assert_eq!(
        document::parse(r#"{"a":true,"b":1.0}"#).unwrap(),
        document::parse(r#"{"b":1,"a":1}"#).unwrap()
    );
}
#[test]
fn failure_bodies_use_python_ascii_json() {
    let mut err = everledger::Failure::usage("bad request");
    err.body = Some(document::parse(r#"{"status":400,"response":{"z":"é","a":1.0}}"#).unwrap());
    let mut out = vec![];
    everledger::cli::write_failure(
        &err,
        &everledger::diagnostics::WriterDiagnostics::new(&mut out),
    )
    .unwrap();
    assert_eq!(out, b"bad request\n{\"z\": \"\\u00e9\", \"a\": 1.0}\n");
}
fn child(scenario: &str, url: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "network_child", "--nocapture"])
        .env("FOUNDATION_SCENARIO", scenario)
        .env("FOUNDATION_URL", url);
    for k in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "REQUEST_METHOD",
    ] {
        c.env_remove(k);
    }
    c
}
fn server(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = vec![];
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "fixture received no request");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut req = vec![];
            while !req.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                stream.read_exact(&mut b).unwrap();
                req.push(b[0]);
            }
            requests.push(String::from_utf8(req).unwrap());
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (format!("http://{addr}"), handle)
}
fn response(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}
#[test]
fn http_status_mapping_redirects_and_headers() {
    let mut responses = vec![response(302, "Location: /ok?x=%2F\r\n", "")];
    responses.push(response(200, "Last-Modified: date\r\n", "ok"));
    for status in [400, 422, 404, 409, 401, 429, 500] {
        responses.push(response(status, "", r#"{"z":"é","a":1.0}"#));
    }
    responses.push(response(302, "Location: /forbidden\r\n", ""));
    let (url, server) = server(responses);
    let output = child("http", &url).output().unwrap();
    let requests = server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    );
    assert_eq!(requests.len(), 10);
    assert!(requests[0].starts_with("GET /start HTTP/1.1\r\n"));
    assert!(requests[1].starts_with("GET /ok?x=%2F HTTP/1.1\r\n"));
    for req in requests {
        let req = req.to_ascii_lowercase();
        assert!(req.contains("user-agent: everledger\r\n"));
        assert!(req.contains("authorization: bearer token\r\n"));
    }
}
#[test]
fn http_proxy_selection_is_per_scheme() {
    let (url, server) = server(vec![response(200, "", "proxy")]);
    let output = child("proxy", "http://fixture.invalid/query?a=1")
        .env("http_proxy", &url)
        .env("https_proxy", "http://127.0.0.1:1")
        .env("ALL_PROXY", "http://127.0.0.1:1")
        .output()
        .unwrap();
    let requests = server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    );
    assert!(requests[0].starts_with("GET http://fixture.invalid/query?a=1 HTTP/1.1\r\n"));
}
#[test]
fn http_no_proxy_bypasses_the_proxy() {
    let (url, server) = server(vec![response(200, "", "proxy")]);
    let output = child("proxy", &url)
        .env("http_proxy", "http://127.0.0.1:1")
        .env("no_proxy", ".127.0.0.1")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    );
}
#[test]
fn network_child() {
    let diagnostics = everledger::diagnostics::WriterDiagnostics::new(std::io::stderr());
    let Ok(scenario) = std::env::var("FOUNDATION_SCENARIO") else {
        return;
    };
    let url = std::env::var("FOUNDATION_URL").unwrap();
    let req = |url: String| Request::get(url).header("Authorization", "Bearer token");
    match scenario.as_str() {
        #[cfg(feature = "parity-seams")]
        "clock" => {
            let clock = VirtualClock::from_env().unwrap().unwrap();
            clock.sleep(0.1).unwrap();
            clock.sleep(0.2).unwrap();
            clock.sleep(60.0).unwrap();
            assert_eq!(clock.monotonic(), 60.3);
            assert_eq!(
                clock.now(),
                time::instant("2026-11-15T12:01:00.423456Z").unwrap()
            );
        }
        "catalogue" => {
            let mut config = everledger::config::Config::from_values(&Default::default()).unwrap();
            config.data_dir = url.into();
            let store = everledger::catalogue::CatalogueStore::default();
            let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
            store.load(&config, &clock, &diagnostics).unwrap();
            store.load(&config, &clock, &diagnostics).unwrap();
        }
        "proxy" => {
            assert_eq!(UreqHttp.request(&req(url)).unwrap().body, b"proxy");
        }
        "http" => {
            let result = UreqHttp.request(&req(format!("{url}/start"))).unwrap();
            assert_eq!(result.body, b"ok");
            assert_eq!(result.header("Last-Modified"), Some("date"));
            for (status, code) in [
                (400, ExitCode::Usage),
                (422, ExitCode::Usage),
                (404, ExitCode::Error),
                (409, ExitCode::Error),
                (401, ExitCode::Unreachable),
                (429, ExitCode::Unreachable),
                (500, ExitCode::Unreachable),
            ] {
                let error = UreqHttp
                    .request(&req(format!("{url}/{status}")))
                    .unwrap_err();
                assert_eq!(error.code, code);
                assert_eq!(
                    error.body.unwrap().get("status"),
                    Some(&Document::from(i64::from(status)))
                );
            }
            let mut r = req(url);
            r.redirects = everledger::http::Redirects::Refuse;
            assert_eq!(
                UreqHttp.request(&r).unwrap_err().code,
                ExitCode::Unreachable
            );
        }
        "redirect-error" => {
            let error = UreqHttp.request(&req(format!("{url}/start"))).unwrap_err();
            assert_eq!(error.message, format!("{url}/start: HTTP 404"));
        }
        "tls" => {
            assert!(!UreqHttp.request(&req(url)).unwrap().body.is_empty());
        }
        _ => panic!("unknown fixture scenario"),
    }
}
#[derive(Default)]
struct FixtureHttp {
    requests: std::cell::RefCell<Vec<Request>>,
    responses: std::cell::RefCell<std::collections::VecDeque<everledger::http::Response>>,
}
impl FixtureHttp {
    fn with_body(body: &str) -> Self {
        Self {
            responses: std::cell::RefCell::new(std::collections::VecDeque::from([
                everledger::http::Response {
                    body: body.as_bytes().to_vec(),
                    headers: vec![],
                },
            ])),
            ..Default::default()
        }
    }
}
impl Http for FixtureHttp {
    fn request(&self, request: &Request) -> everledger::Result<everledger::http::Response> {
        self.requests.borrow_mut().push(request.clone());
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .expect("fixture HTTP response"))
    }
}
#[test]
fn roster_preserves_query_order_and_decodes_typed_views() {
    #[derive(Debug, serde::Deserialize)]
    struct Roster {
        character: Character,
    }
    #[derive(Debug, serde::Deserialize)]
    struct Character {
        name: String,
    }
    let http = FixtureHttp::with_body(r#"{"z":1.0,"character":{"name":"Slade"},"z":2.0}"#);
    let mut config = everledger::config::Config::from_values(&Default::default()).unwrap();
    config.roster_token = "token".into();
    config.roster_url = "http://fixture".into();
    let payload = everledger::roster::roster::<Roster>(
        &http,
        &config,
        "/events",
        &[
            ("char".into(), "Slade Undertow".into()),
            ("since".into(), "2026-11-14T00:00:00Z".into()),
            ("until".into(), Document::Null),
            ("after".into(), "opaque+/=".into()),
        ],
    )
    .unwrap();
    assert_eq!(payload.view().character.name, "Slade");
    assert_eq!(
        document::dumps(payload.document(), document::JsonMode::Stdout),
        r#"{"z": 2.0, "character": {"name": "Slade"}}"#
    );
    assert_eq!(
        http.requests.borrow()[0].url,
        "http://fixture/events?char=Slade+Undertow&since=1794614400&after=opaque%2B%2F%3D"
    );
    assert_eq!(
        http.requests.borrow()[0].headers,
        [("Authorization".into(), "Bearer token".into())]
    );
    let bad = FixtureHttp::with_body(r#"{"character":{}}"#);
    assert!(
        everledger::roster::roster::<Roster>(&bad, &config, "/characters", &[])
            .unwrap_err()
            .message
            .contains("missing field")
    );
}
#[test]
fn price_table_uses_copper_and_freshness() {
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
    let config = everledger::config::Config::from_values(&Default::default()).unwrap();
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let http = FixtureHttp::with_body(
        "market|version|1794744000|2\n2604:88.5:87.5:709:::::3808\n100:::2:::::",
    );
    let diagnostics = everledger::diagnostics::WriterDiagnostics::default();
    let ctx = everledger::Context {
        diagnostics: &diagnostics,
        clock: &clock,
        http: &http,
        config: &config,
        catalogue: &catalogue,
    };
    let (prices, source) = everledger::market::market_table(&ctx).unwrap();
    assert_eq!(prices[&2604].median_copper, Some(everledger::Copper(88)));
    assert_eq!(prices[&2604].floor_copper, Some(everledger::Copper(88)));
    assert_eq!(prices[&2604].sales_per_day, Some(380.8));
    assert_eq!(prices[&100].median_copper, None);
    assert_eq!(source.view().stale, Some(false));
}
#[test]
fn decode_utf8_json() {
    let text = r#"{"é":1.0}"#;
    for bytes in [
        text.as_bytes().to_vec(),
        [vec![0xef, 0xbb, 0xbf], text.as_bytes().to_vec()].concat(),
    ] {
        assert_eq!(
            everledger::http::decode_body(&bytes).unwrap(),
            document::parse(text).unwrap()
        );
    }
    assert!(everledger::http::decode_body(&[0xff]).is_err());
}
#[test]
fn catalogue_load_retries_missing_files_and_caches_success() {
    let diagnostics = everledger::diagnostics::WriterDiagnostics::default();
    let path =
        std::env::temp_dir().join(format!("everledger-catalogue-test-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let mut config = everledger::config::Config::from_values(&Default::default()).unwrap();
    config.data_dir = path.clone();
    let store = everledger::catalogue::CatalogueStore::default();
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
    assert!(store.load(&config, &clock, &diagnostics).is_err());
    std::fs::write(
        path.join("catalogue.json"),
        include_str!("../tests/fixtures/catalogue-fixture.json"),
    )
    .unwrap();
    assert_eq!(
        store
            .load(&config, &clock, &diagnostics)
            .unwrap()
            .view()
            .professions
            .0
            .len(),
        1
    );
    std::fs::remove_file(path.join("catalogue.json")).unwrap();
    assert_eq!(
        store
            .load(&config, &clock, &diagnostics)
            .unwrap()
            .view()
            .professions
            .0
            .len(),
        1
    );
    std::fs::remove_dir(path).unwrap();
}
#[test]
fn argparse_ascii_integers_are_i64() {
    for (text, expected) in [
        ("1_000", 1000),
        (" +0001 ", 1),
        ("-000", 0),
        ("-9223372036854775808", i64::MIN),
        ("9223372036854775807", i64::MAX),
    ] {
        assert_eq!(everledger::cli::parse_integer(text).unwrap(), expected);
    }
    for bad in ["_1", "1_", "1__2", "²", "٣", "99999999999999999999"] {
        assert!(everledger::cli::parse_integer(bad).is_err());
    }
}
#[cfg(feature = "parity-seams")]
#[test]
fn foreverdb_seam_fails_closed() {
    let config = everledger::config::Config::from_values(&Default::default()).unwrap();
    assert_eq!(
        config.foreverdb_base().unwrap_err().code,
        ExitCode::Unreachable
    );
    let config = everledger::config::Config::from_values(&std::collections::HashMap::from([(
        "EVERLEDGER_SEAM_FOREVERDB_URL".into(),
        "http://127.0.0.1:9999".into(),
    )]))
    .unwrap();
    assert_eq!(config.foreverdb_base().unwrap(), "http://127.0.0.1:9999");
}
#[cfg(feature = "parity-seams")]
#[test]
fn virtual_clock_environment_logs_sleep_without_waiting() {
    let path = std::env::temp_dir().join(format!("everledger-sleeps-{}.log", std::process::id()));
    let output = child("clock", "")
        .env("EVERLEDGER_SEAM_NOW", "2026-11-15T12:00:00.123456Z")
        .env("EVERLEDGER_SEAM_SLEEP_LOG", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "0.1\n0.2\n60.0\n");
    std::fs::remove_file(path).unwrap();
}
#[test]
fn catalogue_age_advisory_prints_once() {
    let path =
        std::env::temp_dir().join(format!("everledger-old-catalogue-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("catalogue.json"),
        r#"{"generated_at":"2020-01-01T00:00:00Z","professions":{}}"#,
    )
    .unwrap();
    let output = child("catalogue", path.to_str().unwrap()).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("catalogue is older than 14d; run: everledger refresh catalogue")
            .count(),
        1
    );
    std::fs::remove_file(path.join("catalogue.json")).unwrap();
    std::fs::remove_dir(path).unwrap();
}
#[test]
fn invalid_document_mutations_return_failure() {
    let mut document = Document::Null;
    assert_eq!(document.set("x", 1i64).unwrap_err().code, ExitCode::Error);
    assert!(document.set_default("x", 1i64).is_err());
    assert_eq!(document, Document::Null);
}
#[test]
fn payload_updates_commit_document_and_typed_view_together() {
    #[derive(Debug, serde::Deserialize)]
    struct View {
        stale: bool,
        #[serde(flatten)]
        rest: Other,
    }
    #[derive(Debug, serde::Deserialize)]
    struct Other {
        value: everledger::primitives::Scalar,
    }
    let mut payload = everledger::http::Payload::<View>::decode(
        document::parse(r#"{"stale":false,"value":1.5}"#).unwrap(),
        "fixture",
    )
    .unwrap();
    assert!(matches!(
        payload.view().rest.value,
        everledger::primitives::Scalar::Float(1.5)
    ));
    let before = payload.document().clone();
    assert!(payload.set("stale", "bad").is_err());
    assert_eq!(payload.document(), &before);
    assert!(!payload.view().stale);
    payload.set("stale", true).unwrap();
    assert!(payload.view().stale);
    assert_eq!(payload.document().get("stale"), Some(&Document::Bool(true)));
}
#[test]
fn warnings_survive_command_failure() {
    let args =
        everledger::cli::parse_from(["everledger", "margins", "--known", "X", "--char", "Y"])
            .unwrap();
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
    let config = everledger::config::Config::from_values(&Default::default()).unwrap();
    let catalogue = everledger::catalogue::CatalogueStore::from_document(
        document::parse(include_str!("../tests/fixtures/catalogue-fixture.json")).unwrap(),
    )
    .unwrap();
    let diagnostics = everledger::diagnostics::WriterDiagnostics::default();
    let context = everledger::Context {
        clock: &clock,
        http: &UreqHttp,
        config: &config,
        catalogue: &catalogue,
        diagnostics: &diagnostics,
    };
    let mut stdout = vec![];
    assert_eq!(
        everledger::cli::execute(&args, &context, &mut stdout),
        ExitCode::Usage
    );
    assert!(stdout.is_empty());
    assert_eq!(
        diagnostics.contents(),
        b"deprecated --known; use --char\n--known conflicts with --char\n"
    );
}
#[test]
fn empty_token_file_is_unset() {
    let config = everledger::config::Config::from_values(&std::collections::HashMap::from([(
        "EVERLEDGER_ROSTER_TOKEN_FILE".into(),
        String::new(),
    )]))
    .unwrap();
    assert!(config.roster_token_file.is_none());
    assert_eq!(
        config.roster_token().unwrap_err().code,
        ExitCode::Unreachable
    );
}
#[cfg(unix)]
#[test]
fn non_unicode_environment_does_not_panic() {
    use std::os::unix::ffi::OsStringExt;
    let (url, server) = server(vec![response(503, "", "{}")]);
    let output = Command::new(env!("CARGO_BIN_EXE_everledger"))
        .env_remove("EVERLEDGER_FOREVERDB_DAILY_REQUESTS")
        .args(["price", "2604"])
        .env("EVERLEDGER_AHLEDGER_URL", &url)
        .env("EVERLEDGER_VENDOR_URL", &url)
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1")
        .env(
            "UNRELATED_NON_UNICODE",
            std::ffi::OsString::from_vec(vec![255]),
        )
        .output()
        .unwrap();
    assert_eq!(server.join().unwrap().len(), 1);
    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stderr).contains("HTTP 503"));
}
#[test]
fn redirect_error_names_original_request() {
    let (url, server) = server(vec![
        response(302, "Location: /missing\r\n", ""),
        response(404, "", "{}"),
    ]);
    let output = child("redirect-error", &url).output().unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn checked_instants_cover_python_range() {
    let min = time::instant("0001-01-01T00:00:00Z").unwrap();
    assert!(min.add_microseconds(-1).is_err());
    let max = time::instant("9999-12-31T23:59:59.999999Z").unwrap();
    assert_eq!(time::iso(max), "9999-12-31T23:59:59Z");
    assert!(max.add_microseconds(1).is_err());
    let clock = VirtualClock::new(min);
    assert!(clock.sleep(f64::MAX).is_err());
    assert_eq!(clock.now(), min);
    assert_eq!(clock.monotonic(), 0.0);
}
#[test]
fn nesting_overflow_is_a_failure() {
    let input = format!("{}0{}", "[".repeat(10000), "]".repeat(10000));
    assert_eq!(document::parse(&input).unwrap_err().code, ExitCode::Error);
}
#[test]
fn configured_credentials_cannot_be_formatted() {
    let config = everledger::config::Config::from_values(
        &[
            ("EVERLEDGER_ROSTER_TOKEN", "roster-credential"),
            ("EVERLEDGER_FOREVERDB_API_KEY", "foreverdb-credential"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    )
    .unwrap();
    let printed = format!("{config:?}");
    assert!(!printed.contains("roster-credential"), "{printed}");
    assert!(!printed.contains("foreverdb-credential"), "{printed}");
    assert!(printed.contains("Secret(redacted)"));
    assert_eq!(config.roster_token.expose(), "roster-credential");
}
