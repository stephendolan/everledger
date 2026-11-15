use everledger::{
    clock::VirtualClock,
    document::{self, Document, JsonMode},
    primitives as py, time,
};
fn vectors(name: &str) -> Vec<serde_json::Value> {
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/rust-tests/golden/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn json_modes_and_roundtrips() {
    for v in vectors("json") {
        let d = document::parse(v["payload"].as_str().unwrap()).unwrap();
        for (k, m) in [
            ("stdout", JsonMode::Stdout),
            ("error", JsonMode::Error),
            ("cursor", JsonMode::Cursor),
            ("catalogue", JsonMode::Catalogue),
        ] {
            assert_eq!(document::dumps(&d, m), v[k].as_str().unwrap(), "{v}");
        }
    }
}
#[test]
fn numbers() {
    for v in vectors("numbers") {
        let d = document::parse(&v["value"].to_string()).unwrap();
        assert_eq!(py::py_str(&d).unwrap(), v["text"].as_str().unwrap(), "{v}");
        assert_eq!(
            py::human(d.decode::<Option<py::Scalar>>("scalar").unwrap().as_ref()),
            v["human"].as_str().unwrap()
        );
        if let Some(expected) = v.get("rounded") {
            let copper = py::rounded_copper(d.as_f64().unwrap()).unwrap();
            assert_eq!(copper.0, expected.as_i64().unwrap());
            let expected = v["money"].as_str().unwrap();
            assert_eq!(py::copper_money(Some(copper)), expected);
        }
    }
}
#[test]
fn unicode_and_urls() {
    for v in vectors("strings") {
        let s = v["value"].as_str().unwrap();
        assert_eq!(py::casefold(s), v["casefold"].as_str().unwrap());
        assert_eq!(py::lower(s), v["lower"].as_str().unwrap(), "{s}");
        assert_eq!(py::quote(s), v["quote"].as_str().unwrap());
        assert_eq!(
            py::urlencode(&[
                ("q".into(), Document::from(s)),
                ("n".into(), true.into()),
                ("f".into(), 1.0.into())
            ])
            .unwrap(),
            v["urlencode"].as_str().unwrap()
        );
    }
}
#[test]
fn base64() {
    for v in vectors("base64") {
        assert_eq!(
            py::base64_encode(v["value"].as_str().unwrap().as_bytes()),
            v["encoded"].as_str().unwrap()
        );
    }
    for v in vectors("decode") {
        let result = py::base64_decode(v["value"].as_str().unwrap());
        if v["decoded"].is_null() {
            assert!(result.is_err(), "{v}");
        } else {
            assert_eq!(
                result.unwrap(),
                v["decoded"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u8)
                    .collect::<Vec<_>>(),
                "{v}"
            );
        }
    }
}
#[test]
fn python_time() {
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00.123456Z").unwrap());
    for v in vectors("time") {
        let s = v["value"].as_str().unwrap();
        let zone = Some(v["tz"].as_str().unwrap());
        let parsed = time::instant_in(s, zone);
        if v["instant"].is_null() {
            assert!(parsed.is_err(), "{v}");
        } else {
            let t = parsed.unwrap_or_else(|e| panic!("{v}: {e}"));
            assert_eq!(time::iso(t), v["iso"].as_str().unwrap(), "{v}");
            assert_eq!(
                time::roster_seconds_in(&Document::from(s), zone).unwrap().0,
                v["seconds"].as_i64().unwrap(),
                "{v}"
            );
        }
        assert_eq!(
            time::age_in(Some(s), &clock, zone),
            v["age"].as_i64(),
            "{v}"
        );
        assert_eq!(
            time::time_selector_in(s, &clock, zone).ok().as_deref(),
            v["selector"].as_str(),
            "{v}"
        );
    }
}
#[test]
fn randomized_floats() {
    for v in vectors("float_roundtrip") {
        let d = document::parse(v["payload"].as_str().unwrap()).unwrap();
        assert_eq!(
            document::dumps(&d, JsonMode::Stdout),
            v["stdout"].as_str().unwrap(),
            "{v}"
        );
    }
}
#[test]
fn provenance_rendering() {
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00.123456Z").unwrap());
    for v in vectors("emit") {
        let data = document::parse(&v["data"].to_string()).unwrap();
        let families = v["families"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, rows)| {
                let d = document::parse(&format!("{{\"sources\":{rows}}}")).unwrap();
                (k.clone(), everledger::provenance::sources(&d).unwrap())
            })
            .collect();
        assert_eq!(
            everledger::human::emit(&data, &["row".into()], false, None, &families, &clock)
                .unwrap(),
            v["output"].as_str().unwrap(),
            "{v}"
        );
    }
}
#[test]
fn verify_and_bridge() {
    use everledger::{Context, catalogue::CatalogueStore, config::Config, http::UreqHttp};
    let clock = VirtualClock::new(time::instant("2026-11-15T12:00:00.123456Z").unwrap());
    let config = Config::from_values(&Default::default());
    for v in vectors("commands") {
        let catalogue = CatalogueStore::from_document(
            document::parse(v["catalogue"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let diagnostics = everledger::diagnostics::WriterDiagnostics::default();
        let context = Context {
            diagnostics: &diagnostics,
            clock: &clock,
            http: &UreqHttp,
            config: &config,
            catalogue: &catalogue,
        };
        let args = std::iter::once("everledger").chain(
            v["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap()),
        );
        let args = everledger::cli::parse_from(args).unwrap();
        let mut stdout = vec![];
        let code = everledger::cli::execute(&args, &context, &mut stdout);
        assert_eq!(code as i64, v["code"].as_i64().unwrap(), "{v}");
        assert_eq!(stdout, v["stdout"].as_str().unwrap().as_bytes(), "{v}");
        assert_eq!(
            diagnostics.contents(),
            v["stderr"].as_str().unwrap().as_bytes(),
            "{v}"
        );
    }
}
#[test]
fn human_values() {
    for v in vectors("human") {
        let d = document::parse(v["payload"].as_str().unwrap()).unwrap();
        assert_eq!(
            py::human(d.decode::<Option<py::Scalar>>("scalar").unwrap().as_ref()),
            v["output"].as_str().unwrap()
        );
    }
}
