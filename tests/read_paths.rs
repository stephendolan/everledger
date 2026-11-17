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
    assert_eq!(a.coverage().rows().len(), 89);
    assert_eq!(
        a.coverage()
            .rows()
            .iter()
            .map(|r| r.coords.len())
            .sum::<usize>(),
        91
    );
    assert_eq!(
        a.coverage()
            .rows()
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
        f.command(&["vendors", "2604"]).unwrap().code,
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
    assert_eq!(f.snapshot().coverage().rows().len(), 89);
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
