use everledger::{
    Context, Copper, ExitCode, Failure, Result,
    catalogue::CatalogueStore,
    clock::VirtualClock,
    config::Config,
    diagnostics::WriterDiagnostics,
    document::{self, Document},
    http::{Http, Request, Response},
    time,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    config: Config,
    clock: VirtualClock,
    catalogue: CatalogueStore,
    http: FixtureHttp,
    diagnostics: WriterDiagnostics<Vec<u8>>,
}
#[derive(Default)]
struct FixtureHttp {
    routes: RefCell<BTreeMap<String, Result<Response>>>,
    requests: RefCell<Vec<String>>,
}
impl Http for FixtureHttp {
    fn request(&self, r: &Request) -> Result<Response> {
        self.requests.borrow_mut().push(r.url.clone());
        self.routes
            .borrow()
            .get(&r.url)
            .cloned()
            .unwrap_or_else(|| Err(failure(404)))
    }
}
fn failure(status: i64) -> Failure {
    let mut e = Failure::new(
        format!("fixture HTTP {status}"),
        if status >= 500 {
            ExitCode::Unreachable
        } else {
            ExitCode::Usage
        },
    );
    e.body = Some(Document::object([("status", Document::from(status))]));
    e
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "everledger-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let config = Config::from_values(
            &[
                (
                    "EVERLEDGER_DATA_DIR".into(),
                    path.to_string_lossy().into_owned(),
                ),
                ("EVERLEDGER_ROSTER_URL".into(), "http://127.0.0.1".into()),
                ("EVERLEDGER_VENDOR_URL".into(), "http://127.0.0.1".into()),
                ("EVERLEDGER_AHLEDGER_URL".into(), "http://127.0.0.1".into()),
                ("EVERLEDGER_ROSTER_TOKEN".into(), "fixture".into()),
            ]
            .into(),
        )
        .unwrap();
        Self {
            config,
            clock: VirtualClock::new(time::instant("2026-11-15T12:00:00.789012Z").unwrap()),
            catalogue: CatalogueStore::from_document(
                document::parse(include_str!("fixtures/catalogue-fixture.json")).unwrap(),
            )
            .unwrap(),
            http: FixtureHttp::default(),
            diagnostics: WriterDiagnostics::default(),
        }
    }
    fn context(&self) -> Context<'_> {
        Context {
            clock: &self.clock,
            http: &self.http,
            config: &self.config,
            catalogue: &self.catalogue,
            diagnostics: &self.diagnostics,
        }
    }
    fn raw(&self, path: &str, body: &str) {
        self.http.routes.borrow_mut().insert(
            format!("http://127.0.0.1{path}"),
            Ok(Response {
                body: body.as_bytes().to_vec(),
                headers: vec![(
                    "Last-Modified".into(),
                    "Sat, 14 Nov 2026 20:20:39 GMT".into(),
                )],
            }),
        );
    }
    fn fail(&self, path: &str, status: i64) {
        self.http
            .routes
            .borrow_mut()
            .insert(format!("http://127.0.0.1{path}"), Err(failure(status)));
    }
    fn command(&self, args: &[&str]) -> Result<everledger::commands::CommandOutput> {
        everledger::cli::dispatch(
            &everledger::cli::parse_from(std::iter::once("everledger").chain(args.iter().copied()))
                .unwrap(),
            &self.context(),
        )
    }
    fn dye(&self) {
        self.raw("/items/2604.json", include_str!("fixtures/2604.json"));
        self.raw(
            "/items/2604.pins.json",
            include_str!("fixtures/2604.pins.json"),
        );
    }
    fn market(&self, reagent: &str) {
        self.raw(
            "/v1/pricetable/forever.pvp.alliance.us",
            &format!("market|v|1794744000|2\n2604:{reagent}:87:709:::::3808\n100:200:190:2:::::10"),
        );
    }
    fn snapshot(&self) -> everledger::vendors::NinjaAnswer {
        everledger::vendors::ninja_lookup(2604, false, &self.context()).unwrap()
    }
    fn cache(&self) -> std::path::PathBuf {
        self.config
            .data_dir
            .join("vendors/wowforever.ninja/2604.json")
    }
    fn age_cache(&self, stamp: &str) {
        let mut d = document::parse(&std::fs::read_to_string(self.cache()).unwrap()).unwrap();
        d.set("fetched_at", stamp).unwrap();
        std::fs::write(
            self.cache(),
            document::dumps(&d, document::JsonMode::Stdout),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.config.data_dir);
    }
}
#[test]
fn three_vendor_absences_survive_zone_filter() {
    let f = Fixture::new();
    f.raw("/items/6532.json", r#"{"name":"Baubles"}"#);
    f.raw("/items/901.json", r#"{"name":"Empty"}"#);
    f.raw("/items/901.pins.json", r#"{"zones":{}}"#);
    for (id, reason, empty) in [
        ("900", Some("item not in dataset"), false),
        ("6532", Some("no pins published"), false),
        ("901", None, true),
    ] {
        let out = f.command(&["vendors", id, "--in", "IF"]).unwrap();
        assert_eq!(out.data.get("reason").and_then(Document::as_str), reason);
        assert_eq!(
            out.data.get("vendors"),
            Some(&if empty {
                Document::Array(vec![])
            } else {
                Document::Null
            })
        );
    }
    assert!(f.command(&["vendors", "6532"]).unwrap().lines[0].contains("coverage unknown"));
}
#[test]
fn red_dye_preserves_all_sellers_pins_and_known_zones() {
    let f = Fixture::new();
    f.dye();
    let a = f.snapshot();
    assert_eq!(a.rows().len(), 89);
    assert_eq!(a.rows().iter().map(|r| r.coords.len()).sum::<usize>(), 91);
    assert_eq!(
        a.rows()
            .iter()
            .find(|r| r.name == "Felika")
            .unwrap()
            .coords
            .len(),
        3
    );
    let out = f.command(&["vendors", "2604", "--in", "IF"]).unwrap();
    let rows = out.data.get("vendors").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter()
            .all(|r| r.get("price_copper") == Some(&Document::from(50_i64)))
    );
    let out = f.command(&["vendors", "2604", "--in", "Nowhere"]).unwrap();
    assert!(out.lines.iter().any(|l| l.contains("Ironforge")));
}
#[test]
fn vendor_provenance_and_unparseable_price_are_preserved() {
    let f = Fixture::new();
    f.raw("/items/22.json", r#"{"name":"Fixture"}"#);
    f.raw("/items/22.pins.json",r#"{"zones":{"test":{"vendors":{"1":{"n":"Changed","s":"Supply · nonsense","f":"u"},"2":{"n":"Unknown","s":"Supply · 1g 2s 3c","f":"r"}},"pins":{"vendor":[[1,2,1],[3,4,2]]},"codes":{"vendor":["c","r"]}}}}"#);
    let out = f.command(&["vendors", "22"]).unwrap();
    let rows = out.data.get("vendors").unwrap().as_array().unwrap();
    assert_eq!(rows[0].get("price_copper"), Some(&Document::Null));
    assert_eq!(rows[0].get("status_raw"), Some(&Document::from("c")));
    assert_eq!(rows[0].get("status"), Some(&Document::from("changed")));
    assert_eq!(
        rows[0].get("raw").unwrap().get("f"),
        Some(&Document::from("u"))
    );
    assert_eq!(
        rows[1].get("price_copper"),
        Some(&Document::from(10203_i64))
    );
    assert_eq!(rows[1].get("basis"), Some(&Document::from("unverified")));
    assert!(out.lines.iter().any(|l| l.contains("listed price unknown")));
}
#[test]
fn vendor_price_grammar_never_mistakes_unknown_for_zero() {
    for raw in [
        "",
        "nonsense",
        "1s 2s",
        "1s junk",
        "1 s",
        "-1c",
        "9223372036854775808g",
    ] {
        assert_eq!(everledger::vendors::parse_price(raw), None);
    }
    assert_eq!(everledger::vendors::parse_price("0c"), Some(Copper(0)));
    assert_eq!(
        everledger::vendors::parse_price(" 1g2s 3c "),
        Some(Copper(10203))
    );
}
#[test]
fn cache_bytes_precede_runtime_metadata_and_cache_is_served_without_refetch() {
    let f = Fixture::new();
    f.dye();
    let first = f.snapshot();
    let bytes = std::fs::read_to_string(f.cache()).unwrap();
    assert!(!bytes.contains("\"stale\""));
    assert!(!bytes.contains("\"sources\""));
    assert!(bytes.contains("\"raw_pins\": {"));
    assert!(!bytes.ends_with('\n'));
    let mut document = first.payload().document().clone();
    document.remove("stale");
    document.remove("sources");
    assert_eq!(
        bytes,
        document::dumps(&document, document::JsonMode::Stdout)
    );
    assert_eq!(f.http.requests.borrow().len(), 2);
    f.snapshot();
    assert_eq!(f.http.requests.borrow().len(), 2);
    f.age_cache("2026-11-13T12:00:00Z");
    assert_eq!(f.snapshot().payload().view().stale, Some(true));
    assert_eq!(
        f.command(&["vendors", "2604"]).unwrap().exit_code(),
        ExitCode::Stale
    );
}
#[test]
fn cache_retains_stale_only_for_seven_days_and_server_failures() {
    let f = Fixture::new();
    f.dye();
    f.snapshot();
    f.fail("/items/2604.json", 503);
    f.age_cache("2026-11-13T12:00:00Z");
    let stale = everledger::vendors::ninja_lookup(2604, true, &f.context()).unwrap();
    assert_eq!(stale.payload().view().stale, Some(true));
    f.age_cache("2026-11-07T12:00:00Z");
    assert_eq!(
        everledger::vendors::ninja_lookup(2604, true, &f.context())
            .unwrap_err()
            .code,
        ExitCode::Unreachable
    );
    f.age_cache("2026-11-13T12:00:00Z");
    f.fail("/items/2604.json", 400);
    assert_eq!(
        everledger::vendors::ninja_lookup(2604, true, &f.context())
            .unwrap_err()
            .code,
        ExitCode::Usage
    );
}
#[test]
fn pins_last_modified_takes_precedence() {
    let f = Fixture::new();
    f.dye();
    let mut routes = f.http.routes.borrow_mut();
    routes
        .get_mut("http://127.0.0.1/items/2604.json")
        .unwrap()
        .as_mut()
        .unwrap()
        .headers = vec![(
        "Last-Modified".into(),
        "Fri, 13 Nov 2026 20:20:39 GMT".into(),
    )];
    drop(routes);
    assert_eq!(
        f.snapshot().payload().view().published_at.as_deref(),
        Some("2026-11-14T20:20:39Z")
    );
}
#[test]
fn margin_omits_unknown_reagent_but_keeps_a_zero_cost() {
    let f = Fixture::new();
    f.market("");
    assert!(
        f.command(&["margins"])
            .unwrap()
            .data
            .get("margins")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.market("0");
    let out = f.command(&["margins"]).unwrap();
    let row = &out.data.get("margins").unwrap().as_array().unwrap()[0];
    assert_eq!(row.get("cost_copper"), Some(&Document::from(0_i64)));
    assert_eq!(row.get("margin_copper"), Some(&Document::from(190_i64)));
    f.market("88");
    let out = f.command(&["margins"]).unwrap();
    let row = &out.data.get("margins").unwrap().as_array().unwrap()[0];
    assert_eq!(row.get("margin_copper"), Some(&Document::from(102_i64)));
}
#[test]
fn unavailable_recipe_knowledge_errors_named_profession_and_skips_unspecified() {
    let f = Fixture::new();
    f.market("88");
    f.raw("/characters/Slade",r#"{"character":{"professions":[{"name":"Tailoring","skill":5,"skill_available":true,"recipes":{"available":false,"known":null}}]}}"#);
    assert_eq!(
        f.command(&["margins", "tailoring", "--char", "Slade"])
            .err()
            .unwrap()
            .code,
        ExitCode::Error
    );
    assert!(
        f.command(&["margins", "--char", "Slade"])
            .unwrap()
            .data
            .get("margins")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.raw("/characters/Slade",r#"{"character":{"professions":[{"name":"Tailoring","skill":5,"skill_available":true,"recipes":{"available":true,"known":[]}}]}}"#);
    assert!(
        f.command(&["margins", "tailoring", "--char", "Slade"])
            .unwrap()
            .data
            .get("margins")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn unavailable_skill_requires_override_even_with_known_empty_recipes() {
    let f = Fixture::new();
    f.market("88");
    f.raw("/characters/Slade",r#"{"character":{"professions":[{"name":"Tailoring","skill_available":false,"recipes":{"available":true,"known":[]}}]}}"#);
    assert_eq!(
        f.command(&["margins", "--char", "Slade"])
            .err()
            .unwrap()
            .code,
        ExitCode::Usage
    );
    assert!(
        f.command(&["margins", "--char", "Slade", "--level", "5"])
            .is_ok()
    );
}
#[test]
fn recipe_roster_failure_degrades_and_known_empty_is_distinct_from_unavailable() {
    let f = Fixture::new();
    f.fail("/characters", 503);
    let out = f.command(&["recipe", "Red Shirt"]).unwrap();
    assert_eq!(out.code, ExitCode::Answer);
    assert!(out.lines.iter().any(|l| l.contains("not checked")));
    assert_eq!(
        out.data.get("recipes").unwrap().as_array().unwrap()[0].get("known_by"),
        Some(&Document::Null)
    );
    f.raw("/characters",r#"{"characters":[{"name":"Empty","professions":[{"name":"Tailoring","recipes":{"available":true,"known":[]}}]},{"name":"Unknown"}],"sources":[]}"#);
    let out = f.command(&["recipe", "Red Shirt"]).unwrap();
    let row = &out.data.get("recipes").unwrap().as_array().unwrap()[0];
    assert_eq!(row.get("known_by"), Some(&Document::Array(vec![])));
    assert_eq!(
        row.get("recipe_data_unavailable"),
        Some(&Document::Array(vec![Document::from("Unknown")]))
    );
}
#[test]
fn margin_exit_considers_roster_and_market_sources() {
    let f = Fixture::new();
    f.market("88");
    f.raw("/characters/Slade",r#"{"character":{"professions":[{"name":"Tailoring","skill":5,"skill_available":true,"recipes":{"available":true,"known":[]}}]},"sources":[{"name":"roster","stale":true}]}"#);
    assert!(f.command(&["margins", "--char", "Slade"]).unwrap().stale);
    f.raw(
        "/v1/pricetable/forever.pvp.alliance.us",
        "m|v|1\n2604:88:87:2:::::10\n100:200:190:2:::::10",
    );
    assert!(f.command(&["margins"]).unwrap().stale);
    f.fail("/v1/pricetable/forever.pvp.alliance.us", 503);
    assert_eq!(
        f.command(&["margins"]).err().unwrap().code,
        ExitCode::Unreachable
    );
}
#[test]
fn price_uses_half_even_copper_and_preserves_unknown_counts() {
    let f = Fixture::new();
    f.raw("/v1/prices/forever.pvp.alliance.us/42",r#"{"item":{"id":42,"name":"Example"},"latest":{"observedAt":"2026-11-15T12:00:00Z","median":88.5,"minBuyout":87.5,"median7d":null,"quantity":null,"auctions":0},"sales":{"estimatedPerDay":null,"coveredHours":null}}"#);
    let out = f.command(&["price", "42"]).unwrap();
    let latest = out.data.get("latest").unwrap();
    assert_eq!(latest.get("median_copper"), Some(&Document::from(88_i64)));
    assert_eq!(
        latest.get("min_buyout_copper"),
        Some(&Document::from(88_i64))
    );
    assert_eq!(latest.get("median_7d_copper"), Some(&Document::Null));
    assert!(out.lines[0].contains("listed unknown in 0 auctions"));
    assert_eq!(out.code, ExitCode::Answer);
    let mut d = document::parse(
        std::str::from_utf8(
            &f.http
                .routes
                .borrow()
                .get("http://127.0.0.1/v1/prices/forever.pvp.alliance.us/42")
                .unwrap()
                .as_ref()
                .unwrap()
                .body,
        )
        .unwrap(),
    )
    .unwrap();
    let mut latest = d.get("latest").unwrap().clone();
    latest.set("observedAt", Document::Null).unwrap();
    d.set("latest", latest).unwrap();
    f.raw(
        "/v1/prices/forever.pvp.alliance.us/42",
        &document::dumps(&d, document::JsonMode::Stdout),
    );
    assert!(f.command(&["price", "42"]).unwrap().stale);
    f.fail("/v1/prices/forever.pvp.alliance.us/42", 503);
    assert_eq!(
        f.command(&["price", "42"]).err().unwrap().code,
        ExitCode::Unreachable
    );
}
#[test]
fn removed_flags_and_aliases_are_rejected() {
    for args in [vec!["verify"], vec!["margins", "--known", "Slade"]] {
        let error =
            everledger::cli::parse_from(std::iter::once("everledger").chain(args)).unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }
    let f = Fixture::new();
    f.raw(
        "/characters/Slade",
        r#"{"character":{"name":"Slade","professions":[]}}"#,
    );
    let out = f.command(&["char", "Slade", "auctions"]).unwrap();
    assert_eq!(out.code, ExitCode::Answer);
    assert!(f.diagnostics.contents().is_empty());
}

#[test]
fn roster_derived_results_share_stale_notice_and_exit_code() {
    let f = Fixture::new();
    f.market("88");
    let ch = r#"{"name":"Slade","professions":[{"name":"Tailoring","skill":5,"max_skill":75,"skill_available":true,"recipes":{"available":true,"known":[{"spell_id":1}]}}]}"#;
    let source =
        r#"[{"file":"Journalator-looking-name.lua","mtime":"2026-11-15T12:00:00Z","stale":true}]"#;
    f.raw(
        "/characters",
        &format!("{{\"characters\":[{ch}],\"sources\":{source}}}"),
    );
    f.raw(
        "/characters/Slade",
        &format!("{{\"character\":{ch},\"sources\":{source}}}"),
    );
    for args in [
        vec!["recipe", "Red Shirt"],
        vec!["char", "Slade"],
        vec!["margins", "tailoring", "--char", "Slade"],
    ] {
        for json in [false, true] {
            let mut argv = args.clone();
            if json {
                argv.push("--json");
            }
            let command =
                everledger::cli::parse_from(std::iter::once("everledger").chain(argv)).unwrap();
            let mut stdout = vec![];
            assert_eq!(
                everledger::cli::execute(&command, &f.context(), &mut stdout),
                ExitCode::Stale,
                "{args:?}"
            );
            let stdout = String::from_utf8(stdout).unwrap();
            if json {
                document::parse(&stdout).unwrap();
            } else {
                let lines = stdout.lines().collect::<Vec<_>>();
                assert_eq!(
                    stdout
                        .matches("stale: data exceeds its freshness limit")
                        .count(),
                    1
                );
                assert_eq!(
                    lines[lines.len() - 2],
                    "stale: data exceeds its freshness limit"
                );
                assert!(lines.last().unwrap().starts_with("source:"));
            }
        }
    }
}

fn catalogue_with_recipes(f: &mut Fixture, specs: &[(&str, i64, i64, i64)]) {
    let mut cat = document::parse(include_str!("fixtures/catalogue-fixture.json")).unwrap();
    let profs = cat.get("professions").unwrap();
    let block = profs.get("tailoring").unwrap();
    let template = &block.get("recipes").unwrap().as_array().unwrap()[0];
    let recipes = specs
        .iter()
        .enumerate()
        .map(|(i, (name, learn, yellow, grey))| {
            let mut r = template.clone();
            r.set("name", *name).unwrap();
            r.set("spell_id", i as i64 + 1).unwrap();
            r.set(
                "bands",
                Document::object([
                    ("learn", Document::from(*learn)),
                    ("yellow", Document::from(*yellow)),
                    ("green", Document::from(*grey - 1)),
                    ("grey", Document::from(*grey)),
                ]),
            )
            .unwrap();
            r
        })
        .collect();
    cat.set(
        "professions",
        Document::object([(
            "tailoring",
            Document::object([
                ("stated_count", Document::from(specs.len())),
                ("recipes", Document::Array(recipes)),
            ]),
        )]),
    )
    .unwrap();
    f.catalogue = CatalogueStore::from_document(cat).unwrap();
}
#[test]
fn recipe_prefers_exact_matches_and_limits_substring_matches_to_eight() {
    let mut f = Fixture::new();
    let names = (0..10).map(|i| format!("Shirt {i}")).collect::<Vec<_>>();
    let mut specs = names
        .iter()
        .map(|n| (n.as_str(), 1, 10, 30))
        .collect::<Vec<_>>();
    specs.push(("Shirt", 1, 10, 30));
    catalogue_with_recipes(&mut f, &specs);
    f.raw("/characters", r#"{"characters":[]}"#);
    assert_eq!(
        f.command(&["recipe", "sHiRt"])
            .unwrap()
            .data
            .get("recipes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.command(&["recipe", "Shirt "])
            .unwrap()
            .data
            .get("recipes")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        8
    );
}
#[test]
fn char_pads_columns_sorts_options_and_groups_known_names_four_per_line() {
    let mut f = Fixture::new();
    catalogue_with_recipes(
        &mut f,
        &[
            ("Old", 1, 10, 30),
            ("Newest", 5, 10, 30),
            ("Later", 3, 10, 30),
            ("Fourth", 2, 10, 30),
            ("Fifth", 1, 10, 30),
        ],
    );
    f.raw("/characters/Slade",r#"{"character":{"name":"Slade","professions":[{"name":"Tailoring","skill":5,"max_skill":75,"skill_available":true,"recipes":{"available":true,"known":[{"spell_id":1},{"spell_id":2},{"spell_id":3},{"spell_id":4},{"spell_id":5}]}}]}}"#);
    let out = f.command(&["char", "Slade", "tailoring"]).unwrap();
    let expected = format!(
        "      known {:<34} {:<6} learn {:<4} grey 30  Trainer",
        "Newest", "orange", 5
    );
    assert_eq!(
        out.lines.iter().find(|l| l.contains("grey 30")).unwrap(),
        &expected
    );
    assert!(
        out.lines
            .iter()
            .any(|l| l == "      Fifth, Fourth, Later, Newest")
    );
    assert_eq!(out.lines.last().unwrap(), "      Old");
}
#[test]
fn margin_ranking_uses_color_then_expected_profit_and_keeps_fifteen_stably() {
    let mut f = Fixture::new();
    let names = (0..17).map(|i| format!("Shirt {i:02}")).collect::<Vec<_>>();
    let specs = names
        .iter()
        .map(|n| (n.as_str(), 1, 10, 30))
        .collect::<Vec<_>>();
    catalogue_with_recipes(&mut f, &specs);
    f.market("88");
    let out = f.command(&["margins", "--level", "5"]).unwrap();
    let rows = out.data.get("margins").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 15);
    assert_eq!(rows[0].get("name"), Some(&Document::from("Shirt 00")));
    assert_eq!(rows[14].get("name"), Some(&Document::from("Shirt 14")));
}
#[test]
fn cache_from_another_backend_url_is_discarded_before_decoding_snapshot() {
    let f = Fixture::new();
    f.dye();
    std::fs::create_dir_all(f.cache().parent().unwrap()).unwrap();
    std::fs::write(f.cache(), r#"{"backend_url":"http://127.0.0.1:9"}"#).unwrap();
    assert_eq!(f.snapshot().rows().len(), 89);
}
#[test]
fn margin_order_uses_profit_times_sales_but_negative_margins_ignore_sales() {
    let mut f = Fixture::new();
    catalogue_with_recipes(
        &mut f,
        &[
            ("Big", 1, 10, 30),
            ("Fast", 1, 10, 30),
            ("Yellow", 1, 5, 30),
            ("Small loss", 1, 10, 30),
            ("Big loss", 1, 10, 30),
        ],
    );
    let mut cat = f
        .catalogue
        .load(&f.config, &f.clock, &f.diagnostics)
        .unwrap()
        .document()
        .clone();
    let mut block = cat
        .get("professions")
        .unwrap()
        .get("tailoring")
        .unwrap()
        .clone();
    let mut recipes = block.get("recipes").unwrap().as_array().unwrap().to_vec();
    for (i, r) in recipes.iter_mut().enumerate() {
        r.set(
            "result",
            Document::object([
                ("item_id", Document::from(100_i64 + i as i64)),
                ("name", Document::from("Result")),
                ("qty", Document::from(1_i64)),
            ]),
        )
        .unwrap();
    }
    block.set("recipes", Document::Array(recipes)).unwrap();
    cat.set("professions", Document::object([("tailoring", block)]))
        .unwrap();
    f.catalogue = CatalogueStore::from_document(cat).unwrap();
    f.raw("/v1/pricetable/forever.pvp.alliance.us","m|v|1794744000\n2604:100:90:1:::::10\n100:300:290:2:::::10\n101:200:190:2:::::100\n102:1000:900:2:::::1000\n103:100:90:2:::::10000\n104:50:40:2:::::1");
    let out = f.command(&["margins", "--level", "5"]).unwrap();
    let names = out
        .data
        .get("margins")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.get("name").unwrap().as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Fast", "Big", "Small loss", "Big loss", "Yellow"]);
}
#[test]
fn craft_result_quantity_is_applied_before_auction_cut() {
    let mut f = Fixture::new();
    f.market("88");
    let mut cat = document::parse(include_str!("fixtures/catalogue-fixture.json")).unwrap();
    let mut recipe = cat
        .get("professions")
        .unwrap()
        .get("tailoring")
        .unwrap()
        .get("recipes")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .clone();
    let mut result = recipe.get("result").unwrap().clone();
    result.set("qty", 2_i64).unwrap();
    recipe.set("result", result).unwrap();
    cat.set(
        "professions",
        Document::object([(
            "tailoring",
            Document::object([
                ("stated_count", Document::from(1_i64)),
                ("recipes", Document::Array(vec![recipe])),
            ]),
        )]),
    )
    .unwrap();
    f.catalogue = CatalogueStore::from_document(cat).unwrap();
    let out = f.command(&["margins"]).unwrap();
    assert_eq!(
        out.data.get("margins").unwrap().as_array().unwrap()[0].get("margin_copper"),
        Some(&Document::from(292_i64))
    );
}
#[test]
fn absent_zone_name_falls_back_to_slug() {
    let f = Fixture::new();
    f.raw("/items/22.json", r#"{"name":"Fixture"}"#);
    f.raw(
        "/items/22.pins.json",
        r#"{"zones":{"test":{"zone":{},"vendors":{"1":{"n":"Seller","s":"Supply · 0c"}}}}}"#,
    );
    let out = f.command(&["vendors", "22"]).unwrap();
    assert_eq!(
        out.data.get("vendors").unwrap().as_array().unwrap()[0].get("zone"),
        Some(&Document::from("test"))
    );
}
#[test]
fn ninja_preserves_nullable_coordinates_and_reads_ascii_prices_only() {
    let f = Fixture::new();
    f.raw("/items/22.json", r#"{"name":"Fixture"}"#);
    f.raw("/items/22.pins.json",r#"{"zones":{"test":{"vendors":{"1":{"n":"Seller","s":"Supply · 1s 2c"},"2":{"n":"Other","s":"Supply · ١s ２c"}},"pins":{"vendor":[[null,0,1]]}}}}"#);
    let out = f.command(&["vendors", "22"]).unwrap();
    let rows = out.data.get("vendors").unwrap().as_array().unwrap();
    let row = &rows[1];
    assert_eq!(row.get("price_copper"), Some(&Document::from(102_i64)));
    assert_eq!(rows[0].get("price_copper"), Some(&Document::Null));
    assert_eq!(rows[0].get("price_raw"), Some(&Document::from("١s ２c")));
    assert_eq!(
        row.get("coords"),
        Some(&document::parse("[[null, 0]]").unwrap())
    );
    assert!(out.lines[2].contains("unknown, 0"));
}

#[test]
fn status_probes_one_event_page_and_stale_outranks_unreachable() {
    for stale_kind in ["none", "listing", "market", "vendor", "ignored"] {
        let mut f = Fixture::new();
        f.market("88");
        f.raw("/", "");
        f.fail("/characters", 503);
        f.raw("/events?limit=1", r#"{"entries":[],"has_more":true,"next_cursor":"unread-page","sources":[{"file":"Journalator.lua","stale":true}]}"#);
        let listing_stale = stale_kind == "listing";
        let ignored = stale_kind == "ignored";
        f.raw("/listings", &format!(r#"{{"listings":[],"sources":[{{"name":"other","stale":{ignored}}}],"scans":[{{"pricing":{{"source":{{"file":"scan.lua","stale":{listing_stale}}}}}}}]}}"#));
        if stale_kind == "market" {
            f.raw(
                "/v1/pricetable/forever.pvp.alliance.us",
                "m|v|1\n2604:88:87:709:::::10",
            );
        } else if stale_kind == "vendor" {
            f.config.foreverdb_api_key = "fixture".into();
            let path = f.config.data_dir.join("vendors/foreverdb.net");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("index.json"), r#"{"format":2,"fetched_at":"2026-11-13T12:00:00Z","published_at":null,"record_count":0,"vendor_total":0,"item_count":0,"items":{}}"#).unwrap();
        }
        for json in [false, true] {
            f.http.requests.borrow_mut().clear();
            let argv = if json {
                vec!["everledger", "status", "--json"]
            } else {
                vec!["everledger", "status"]
            };
            let command = everledger::cli::parse_from(argv).unwrap();
            let mut stdout = vec![];
            let stale = ["listing", "market", "vendor"].contains(&stale_kind);
            assert_eq!(
                everledger::cli::execute(&command, &f.context(), &mut stdout),
                if stale {
                    ExitCode::Stale
                } else {
                    ExitCode::Unreachable
                },
                "{stale_kind}"
            );
            let output = String::from_utf8(stdout).unwrap();
            assert_eq!(
                output.matches("stale: data exceeds").count(),
                usize::from(stale && !json)
            );
            let requests = f.http.requests.borrow();
            assert_eq!(
                requests
                    .iter()
                    .filter(|p| p.contains("/events"))
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["http://127.0.0.1/events?limit=1"]
            );
            assert!(
                !requests
                    .iter()
                    .any(|p| p.contains("/api/v1/") || p.contains("/items/"))
            );
        }
    }
}

#[test]
fn price_quantity_is_normalized_to_an_integer_at_the_boundary() {
    let f = Fixture::new();
    for (quantity, expected) in [
        ("709.0", 709_i64),
        ("709.9", 709),
        ("-1.9", -1),
        ("\"709\"", 709),
        ("true", 1),
    ] {
        f.raw("/v1/prices/forever.pvp.alliance.us/42", &format!(r#"{{"item":{{"id":42,"name":"Example"}},"latest":{{"observedAt":"2026-11-15T12:00:00Z","median":88,"minBuyout":87,"median7d":null,"quantity":{quantity},"auctions":1}},"sales":{{"estimatedPerDay":null,"coveredHours":null}}}}"#));
        let out = f.command(&["price", "42"]).unwrap();
        assert_eq!(
            out.data.get("latest").unwrap().get("quantity"),
            Some(&Document::from(expected))
        );
        assert!(out.lines[0].contains(&format!("listed {expected} in")));
        let json: serde_json::Value =
            serde_json::from_str(&document::dumps(&out.data, document::JsonMode::Stdout)).unwrap();
        assert!(json["latest"]["quantity"].is_i64());
    }
}

#[test]
fn vendor_footer_uses_fetch_time_when_publication_and_age_are_null() {
    let f = Fixture::new();
    f.dye();
    f.snapshot();
    let mut cached = document::parse(&std::fs::read_to_string(f.cache()).unwrap()).unwrap();
    cached.set("published_at", Document::Null).unwrap();
    std::fs::write(
        f.cache(),
        document::dumps(&cached, document::JsonMode::Stdout),
    )
    .unwrap();
    let command = everledger::cli::parse_from(["everledger", "vendors", "2604"]).unwrap();
    let mut stdout = vec![];
    assert_eq!(
        everledger::cli::execute(&command, &f.context(), &mut stdout),
        ExitCode::Answer
    );
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(
        stdout
            .lines()
            .last()
            .unwrap()
            .contains("wowforever.ninja pins (saved/data 2026-11-15T12:00:00Z; age 0s)")
    );
}

#[derive(serde::Deserialize)]
struct CapturedCase {
    argv: Vec<String>,
    exit_code: i32,
    stdout: String,
    requests: Vec<String>,
}

/// `tests/golden/captured-roster.json` is what the Python reference prints for the roster
/// responses in `tests/fixtures/captured-roster.json`, which have the keys and types the
/// service sends. `parity/captured_roster.py` writes it and fails when it goes stale.
#[test]
fn captured_roster_shapes_replay_the_reference() {
    let world = document::parse(include_str!("fixtures/captured-roster.json")).unwrap();
    let now = world.get("now").and_then(Document::as_str).unwrap();
    let routes = world.get("routes").unwrap();
    let cases: Vec<CapturedCase> =
        serde_json::from_str(include_str!("golden/captured-roster.json")).unwrap();
    assert_eq!(cases.len(), 22);
    for case in cases {
        let mut f = Fixture::new();
        f.clock = VirtualClock::new(time::instant(now).unwrap());
        for request in &case.requests {
            let path = request.split('?').next().unwrap();
            let body = routes.get(path).unwrap();
            let text = match body.as_str() {
                Some(text) => text.to_owned(),
                None => document::dumps(body, document::JsonMode::Stdout),
            };
            f.raw(request, &text);
        }
        let command = everledger::cli::parse_from(
            std::iter::once("everledger").chain(case.argv.iter().map(String::as_str)),
        )
        .unwrap();
        let mut stdout = Vec::new();
        let code = everledger::cli::execute(&command, &f.context(), &mut stdout);
        assert_eq!(code as i32, case.exit_code, "{:?}", case.argv);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            case.stdout,
            "{:?}",
            case.argv
        );
        let sent = case
            .requests
            .iter()
            .map(|request| format!("http://127.0.0.1{request}"))
            .collect::<Vec<_>>();
        assert_eq!(*f.http.requests.borrow(), sent, "{:?}", case.argv);
    }
}

#[test]
fn inventory_item_fallback_distinguishes_absent_and_null_fields() {
    for (item, selected) in [
        (r#"{"id":null}"#, false),
        (r#"{}"#, true),
        (r#"{"id":null,"name":"rEd DyE"}"#, true),
        (r#"{"id":null,"name":null}"#, false),
    ] {
        let f = Fixture::new();
        f.market("88");
        f.raw("/characters", r#"{"characters":[{"name":"Slade"}]}"#);
        f.raw("/events?limit=500&since=1794139200", r#"{"entries":[]}"#);
        f.raw("/listings?item=2604", r#"{"listings":[]}"#);
        f.raw("/characters/Slade/inventory", &format!(
            r#"{{"bags":{{"available":true,"items":[{{"item_id":2604,"count":4,"item":{item}}}]}}}}"#
        ));
        let output = f
            .command(&["positions", "--item", "2604", "--json"])
            .unwrap();
        assert_eq!(
            output
                .data
                .get("positions")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            usize::from(selected),
            "{item}"
        );
    }
}

#[test]
fn unread_page_values_use_the_service_decode_path() {
    for (value, succeeds) in [
        ("[".repeat(200) + &"]".repeat(200), true),
        ("NaN".into(), false),
        ("[Infinity]".into(), false),
        ("1e400".into(), false),
    ] {
        let f = Fixture::new();
        f.raw(
            "/events?cursor=page",
            &format!(r#"{{"entries":[],"extra":{value}}}"#),
        );
        match f.command(&["events", "--cursor", "page", "--json"]) {
            Ok(output) => {
                assert!(succeeds);
                assert_eq!(
                    output.data.get("extra"),
                    Some(&document::parse(&value).unwrap())
                );
            }
            Err(error) => {
                assert!(!succeeds);
                assert_eq!(
                    error.message,
                    "roster: invalid response shape (expected value)"
                );
            }
        }
    }
}

#[test]
fn char_displays_ranked_talents_and_keeps_the_roster_payload() {
    let f = Fixture::new();
    let raw = r#"{"character":{"name":"Ada Test","talents":{"available":true,
        "captured_at":"2026-11-21T22:10:00Z","trees":[
            {"name":"Combat","points":0,"talents":[]},
            {"name":"Subtlety","points":3,"talents":[
                {"name":"Improved Ambush","rank":3,"max_rank":3}]}]}}}"#;
    f.raw("/characters/Ada", raw);
    let text = f.command(&["char", "Ada"]).unwrap();
    assert!(text.lines.windows(4).any(|lines| lines
        == [
            "  Talents (captured 2026-11-21T22:10:00Z)",
            "    Combat: 0 points",
            "    Subtlety: 3 points",
            "      Improved Ambush 3/3",
        ]));
    let json = f.command(&["char", "Ada", "--json"]).unwrap();
    let expected = document::parse(raw).unwrap();
    assert_eq!(json.data.get("character"), expected.get("character"));

    f.raw(
        "/characters/Ada",
        r#"{"character":{"name":"Ada Test","talents":{
        "available":false,"reason":"No complete talent scan recorded"}}}"#,
    );
    let unavailable = f.command(&["char", "Ada"]).unwrap();
    assert!(
        unavailable
            .lines
            .iter()
            .any(|line| line == "  Talents unavailable (No complete talent scan recorded)")
    );
}

#[test]
fn char_displays_saved_guide_position_and_preserves_the_wire_fields() {
    let f = Fixture::new();
    f.raw(
        "/characters/Ada",
        r#"{"character":{"name":"Ada Test","guide":{"available":true,"guide_group":"Example Guide",
            "guide_subgroup":"Test Route","chapter":"10-11 Harbor","current_step":8,
            "saved_at":"2026-11-15T11:59:00Z","file_age_seconds":60,
            "current":{"chapter":"10-11 Harbor","step":8,"text":["Talk to Dockmaster"],
                "targets":[],"level_tags":[],"chapter_levels":[10,11],"definition":{"line":42}}}}}"#,
    );
    let answer = f.command(&["char", "Ada", "--json"]).unwrap();
    let guide = answer.data.get("character").unwrap().get("guide").unwrap();
    assert_eq!(guide.get("current_step"), Some(&Document::from(8_i64)));
    assert!(answer.lines.iter().any(
        |line| line.contains("10-11 Harbor step 8 | Talk to Dockmaster")
            && line.contains("SavedVariables 2026-11-15T11:59:00Z, 60s old")
    ));

    f.raw(
        "/characters/Ada",
        r#"{"character":{"name":"Ada Test","guide":{"available":false,"reason":"No progress recorded"}}}"#,
    );
    let answer = f.command(&["char", "Ada"]).unwrap();
    assert!(
        answer
            .lines
            .iter()
            .any(|line| line.contains("RestedXP unavailable (No progress recorded)"))
    );
}

#[test]
fn guide_passes_term_and_limit_and_reports_upcoming_or_search_results() {
    let f = Fixture::new();
    let search = r##"{"character":"Ada Test","available":true,"chapter":"10-11 Harbor","current_step":8,
        "query":"Blue Cave","route_available":true,"route_end":"Next chapter unavailable","truncated":false,
        "steps":[{"chapter":"11-12 Blue Cave","step":1,"steps_from_current":3,"text":[],"targets":["Cave Keeper"],
            "level_tags":["#level 11"],"chapter_levels":[11,12],
            "previous":{"chapter":"10-11 Harbor","step":10,"text":["Leave the harbor"],"targets":[],
                "level_tags":[],"chapter_levels":[10,11]}}]}"##;
    f.raw("/characters/Ada%20Test/guide?q=Blue+Cave&limit=3", search);
    let answer = f
        .command(&["guide", "Ada Test", "--search", "Blue Cave", "--limit", "3"])
        .unwrap();
    assert!(
        answer
            .lines
            .iter()
            .any(|line| line.contains("step 1 (+3 installed steps)")
                && line.contains("#level 11")
                && line.contains("Target Cave Keeper"))
    );
    assert!(
        answer
            .lines
            .iter()
            .any(|line| line.contains("Before this | 10-11 Harbor step 10 | Leave the harbor"))
    );
    assert_eq!(
        answer.data.get("query").and_then(Document::as_str),
        Some("Blue Cave")
    );

    f.raw(
        "/characters/Ada/guide",
        r#"{"character":"Ada Test","available":true,"chapter":"10-11 Harbor",
        "current_step":8,"route_available":true,"steps":[{"chapter":"10-11 Harbor","step":9,
        "steps_from_current":1,"text":["Prepare at the harbor"],"targets":[],"level_tags":[]}]}"#,
    );
    let answer = f.command(&["guide", "Ada"]).unwrap();
    assert!(
        answer
            .lines
            .iter()
            .any(|line| line.contains("step 9 (+1 installed step) | Prepare at the harbor"))
    );
}

#[test]
fn guide_lists_installed_groups_counts_and_unavailable_chapters() {
    let f = Fixture::new();
    f.raw(
        "/guides?group=Dungeon+Guide+1-20",
        include_str!("fixtures/restedxp-guide-catalogue.json"),
    );
    let result = f
        .command(&["guide", "--list", "--group", "Dungeon Guide 1-20", "--json"])
        .unwrap();
    let groups = result.data.get("groups").unwrap().as_array().unwrap();
    let chapters = groups[0].get("chapters").unwrap().as_array().unwrap();
    assert_eq!(chapters[0].get("step_count"), Some(&Document::Null));
    assert_eq!(
        chapters[2].get("step_count"),
        Some(&Document::from(129_i64))
    );
    assert_eq!(
        result.lines,
        [
            "RestedXP Forever Dungeon Guide (A) / Dungeon Guide 1-20",
            "  1-9 Zephras Isle | levels 1–9 | step count unavailable (Chapter uses RestedXP includes)",
            "  18-19 Ruins of Lordaeron | levels 18–19 | 60 defined steps",
            "  19-20 Deadmines | levels 19–20 | 129 defined steps",
        ]
    );
    f.raw(
        "/guides",
        r#"{"available":false,"reason":"No installed guide chapters could be identified","groups":null,
            "sources":[{"file":"damaged.lua","issues":["Cannot read guide file"]}]}"#,
    );
    let result = f.command(&["guide", "--list"]).unwrap();
    assert_eq!(result.data.get("groups"), Some(&Document::Null));
    assert!(result.lines[0].contains("Installed guides unavailable"));
    assert!(result.lines[1].contains("damaged.lua | Cannot read guide file"));
    for args in [
        &["guide"][..],
        &["guide", "Ada", "--list"],
        &["guide", "Ada", "--group", "Test"],
        &["guide", "--group", "Test"],
        &["guide", "--list", "--search", "Cave"],
        &["guide", "--list", "--limit", "1"],
    ] {
        assert!(
            everledger::cli::parse_from(std::iter::once("everledger").chain(args.iter().copied()))
                .is_err(),
            "accepted {args:?}"
        );
    }
}

fn parity_fixture(epoch: i64) -> Fixture {
    let mut fixture = Fixture::new();
    let recipe = |spell_id, name, input_id, input_qty, output_id, output_qty| {
        serde_json::json!({
            "spell_id": spell_id,
            "name": name,
            "bands": {"learn": 1, "yellow": null, "green": null, "grey": null},
            "reagents": [{"item_id": input_id, "qty": input_qty, "name": "Input"}],
            "result": {"item_id": output_id, "qty": output_qty, "name": name},
            "source_type": "Trainer"
        })
    };
    fixture.catalogue = CatalogueStore::from_document(
        Document::from_typed(&serde_json::json!({
            "generated_at": "2026-11-15T12:00:00Z",
            "source": "fixture catalogue",
            "professions": {"tailoring": {"stated_count": 6, "recipes": [
                recipe(1, "Bulk cloth", 10, 2, 11, 3),
                recipe(2, "Single cloth", 10, 2, 12, 1),
                recipe(3, "Median only", 10, 1, 13, 1),
                recipe(4, "Bandage", 10, 3, 14, 1),
                recipe(5, "No floor", 10, 1, 15, 1),
                recipe(6, "Unsupported quantity", 10, 0, 12, 1)
            ]}}
        }))
        .unwrap(),
    )
    .unwrap();
    fixture.raw(
        "/v1/pricetable/forever.pvp.alliance.us",
        &format!(
            "AHL1|forever/pvp/alliance/us|{epoch}|8\n\
             10:125:100:40:::::0\n\
             11:100:100:4:::::100\n\
             12:250:245:3:::::100\n\
             13:200:100:2:::::100\n\
             14:10:1::::::\n\
             15:1000::1:::::100\n\
             10938:150:141:80:::::300\n\
             10939:468:330:32:::::100"
        ),
    );
    fixture
}

#[test]
fn parity_ranks_realized_spread_per_unit_and_keeps_both_price_bases() {
    let f = parity_fixture(1794744000);
    let command = everledger::cli::parse_from(["everledger", "parity", "--json"]).unwrap();
    let mut stdout = Vec::new();
    assert_eq!(
        everledger::cli::execute(&command, &f.context(), &mut stdout),
        ExitCode::Answer
    );
    let json: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    let rows = json["parities"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "Split Greater Magic Essence",
            "Single cloth",
            "Bulk cloth",
            "Median only"
        ]
    );
    let split = &rows[0];
    assert_eq!(split["source"]["kind"], "item_use");
    assert!(
        split["source"]["url"]
            .as_str()
            .unwrap()
            .contains("10939-greater-magic-essence")
    );
    assert_eq!(split["output"]["qty"], 3);
    assert_eq!(split["floor_realized"]["output_value_copper"], 399);
    assert_eq!(split["floor_realized"]["spread_copper"], 69);
    assert!((split["floor_realized"]["spread_percent"].as_f64().unwrap() - 20.909).abs() < 0.001);
    assert_eq!(split["weighted_spread_copper_per_day"], 690.0);
    assert_eq!(split["output_listed_units"], 80);
    assert_eq!(split["output_sales_per_day"], 30.0);
    assert_eq!(split["verdict"], "positive at floors");

    let bulk = &rows[2];
    assert_eq!(
        bulk["source"],
        serde_json::json!({"kind":"catalogue","profession":"tailoring","spell_id":1})
    );
    assert_eq!(
        bulk["floor"],
        serde_json::json!({
            "input_cost_copper": 200, "output_value_copper": 300,
            "spread_copper": 100, "spread_percent": 50.0
        })
    );
    assert_eq!(
        bulk["median"],
        serde_json::json!({
            "input_cost_copper": 250, "output_value_copper": 300,
            "spread_copper": 50, "spread_percent": 20.0
        })
    );
    assert_eq!(bulk["floor_realized"]["spread_copper"], 82);
    assert_eq!(bulk["median_after_cut"]["spread_copper"], 35);
    assert_eq!(rows[3]["verdict"], "positive only at medians");

    let all = f.command(&["parity", "--json", "--all"]).unwrap();
    let json: serde_json::Value = all.data.decode("parity output").unwrap();
    let rows = json["parities"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let combine = rows
        .iter()
        .find(|r| r["name"] == "Combine Lesser Magic Essence")
        .unwrap();
    assert_eq!(combine["inputs"][0]["qty"], 3);
    assert_eq!(combine["output"]["qty"], 1);
    assert_eq!(combine["median"]["spread_copper"], 18);
    assert_eq!(combine["verdict"], "negative both");
    let bandage = rows.last().unwrap();
    assert_eq!(bandage["name"], "Bandage");
    assert_eq!(bandage["verdict"], "negative both");
    assert_eq!(bandage["floor_realized"]["output_value_copper"], 1);
    assert!(bandage["output_sales_per_day"].is_null());
    assert!(bandage["weighted_spread_copper_per_day"].is_null());
    assert_eq!(
        *f.http.requests.borrow(),
        ["http://127.0.0.1/v1/pricetable/forever.pvp.alliance.us"; 2]
    );
    assert!(!f.config.data_dir.exists());
}

#[test]
fn parity_prints_stale_rows_and_propagates_source_failure() {
    let f = parity_fixture(1794743099);
    let command = everledger::cli::parse_from(["everledger", "parity", "--all"]).unwrap();
    let mut stdout = Vec::new();
    assert_eq!(
        everledger::cli::execute(&command, &f.context(), &mut stdout),
        ExitCode::Stale
    );
    let table = String::from_utf8(stdout).unwrap();
    assert!(table.contains("Realized spread"));
    assert!(table.contains("Bandage"));
    assert!(table.contains("negative both"));
    assert!(table.contains("stale:"));
    assert!(table.contains("2026-11-15T11:44:59Z"));
    assert!(table.contains("source:"));

    f.fail("/v1/pricetable/forever.pvp.alliance.us", 503);
    let mut stdout = Vec::new();
    assert_eq!(
        everledger::cli::execute(&command, &f.context(), &mut stdout),
        ExitCode::Unreachable
    );
    assert!(stdout.is_empty());
}
