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
    assert_eq!(
        args.get_one::<everledger::types::CliInteger>("limit"),
        Some(&(-1).into())
    );
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
    everledger::cli::write_failure(&err, &mut out).unwrap();
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
            let mut config = everledger::config::Config::from_values(&Default::default());
            config.data_dir = url.into();
            let store = everledger::catalogue::CatalogueStore::default();
            let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
            store.load(&config, &clock).unwrap();
            store.load(&config, &clock).unwrap();
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
    let mut config = everledger::config::Config::from_values(&Default::default());
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
    assert_eq!(payload.view.character.name, "Slade");
    assert_eq!(
        document::dumps(&payload.document, document::JsonMode::Stdout),
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
    let config = everledger::config::Config::from_values(&Default::default());
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let http = FixtureHttp::with_body(
        "market|version|1794744000|2\n2604:88.5:87.5:709:::::3808\n100:::2:::::",
    );
    let ctx = everledger::Context {
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
    assert_eq!(source.view.stale, Some(false));
}
#[test]
fn decode_json_byte_encodings() {
    let text = r#"{"é": 1.0}"#;
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    for bytes in [
        text.as_bytes().to_vec(),
        utf16.iter().flat_map(|u| u.to_le_bytes()).collect(),
        utf16.iter().flat_map(|u| u.to_be_bytes()).collect(),
        text.chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect(),
        text.chars()
            .flat_map(|c| (c as u32).to_be_bytes())
            .collect(),
    ] {
        assert_eq!(
            everledger::http::decode_body(&bytes).unwrap(),
            document::parse(text).unwrap()
        );
    }
}
#[test]
fn catalogue_load_retries_missing_files_and_caches_success() {
    let path =
        std::env::temp_dir().join(format!("everledger-catalogue-test-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let mut config = everledger::config::Config::from_values(&Default::default());
    config.data_dir = path.clone();
    let store = everledger::catalogue::CatalogueStore::default();
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00Z").unwrap());
    assert!(store.load(&config, &clock).is_err());
    std::fs::write(
        path.join("catalogue.json"),
        include_str!("../tests/fixtures/catalogue-fixture.json"),
    )
    .unwrap();
    assert_eq!(
        store
            .load(&config, &clock)
            .unwrap()
            .view
            .professions
            .0
            .len(),
        1
    );
    std::fs::remove_file(path.join("catalogue.json")).unwrap();
    assert_eq!(
        store
            .load(&config, &clock)
            .unwrap()
            .view
            .professions
            .0
            .len(),
        1
    );
    std::fs::remove_dir(path).unwrap();
}
#[test]
fn argparse_integers_are_not_machine_limited() {
    for (text, expected) in [
        ("١_٢_٣", "123"),
        (" +0001 ", "1"),
        ("-000", "0"),
        (
            "99999999999999999999999999999999",
            "99999999999999999999999999999999",
        ),
    ] {
        let integer: text_alias::Integer = text.parse().unwrap();
        assert_eq!(integer.to_string(), expected);
        let matches = everledger::cli::parser()
            .try_get_matches_from(["everledger", "events", "--limit", text])
            .unwrap();
        assert_eq!(
            matches
                .subcommand()
                .unwrap()
                .1
                .get_one::<text_alias::Integer>("limit"),
            Some(&integer)
        );
    }
    for bad in ["_1", "1_", "1__2", "²"] {
        assert!(bad.parse::<text_alias::Integer>().is_err());
    }
}
mod text_alias {
    pub type Integer = everledger::types::CliInteger;
}
#[cfg(feature = "parity-seams")]
#[test]
fn foreverdb_seam_fails_closed() {
    let config = everledger::config::Config::from_values(&Default::default());
    assert_eq!(
        config.foreverdb_base().unwrap_err().code,
        ExitCode::Unreachable
    );
    let config = everledger::config::Config::from_values(&std::collections::HashMap::from([(
        "EVERLEDGER_SEAM_FOREVERDB_URL".into(),
        "http://127.0.0.1:9999".into(),
    )]));
    assert_eq!(config.foreverdb_base().unwrap(), "http://127.0.0.1:9999");
    assert_eq!(everledger::config::FOREVERDB_DAILY_REQUESTS, 36);
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
