//! Reviewer-owned probe of the Python-fidelity primitives.
//!
//! `rust-tests/probe/probe.jsonl` is generated from the live oracle by
//! `scripts/gen_oracle_probe.py`, independently of the implementation's own
//! golden vectors. `EVERLEDGER_PROBE` selects another corpus file.

use everledger::{
    Copper,
    clock::VirtualClock,
    document::{Document, JsonMode, dumps, parse},
    primitives, time,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// The wall clock the corpus generator pinned while recording the oracle.
const PROBE_NOW: &str = "2026-11-15T12:34:56.789012Z";

/// Inputs where the oracle's behavior is a CPython artifact the port leaves behind.
const SANCTIONED_TIME_INPUTS: [(&str, &str); 3] = [
    (
        "2026-11-14T10:30:15 Z",
        "CPython accepts one stray character before a UTC offset",
    ),
    (
        "0001-01-01",
        "CPython fails for naive inputs within a day of its minimum date",
    ),
    (
        "9999-12-31",
        "CPython fails for naive inputs within a day of its maximum date",
    ),
];

#[derive(Default)]
struct Tally {
    checked: usize,
    sanctioned: usize,
    failures: Vec<String>,
}

fn document(value: &Value) -> Document {
    parse(&serde_json::to_string(value).unwrap()).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn primitives_match_the_live_oracle() {
    let path = std::env::var("EVERLEDGER_PROBE").unwrap_or_else(|_| {
        format!(
            "{}/rust-tests/probe/probe.jsonl",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let clock = VirtualClock::new(time::instant_in(PROBE_NOW, Some("UTC")).unwrap());
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut unrepresentable = 0usize;

    for line in text.lines() {
        // A record holding an unpaired surrogate cannot be a Rust string at all.
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            unrepresentable += 1;
            continue;
        };
        let kind = record["kind"].as_str().unwrap().to_owned();
        let text_of = |key: &str| record[key].as_str().map(str::to_owned);
        let sanction = record["input"].as_str().and_then(|input| {
            SANCTIONED_TIME_INPUTS
                .iter()
                .find(|(sanctioned, _)| {
                    *sanctioned == input
                        && matches!(kind.as_str(), "instant" | "time_selector" | "age")
                })
                .map(|(_, reason)| *reason)
        });
        let mut check = |label: String, actual: String, expected: String| {
            let tally = tallies.entry(kind.clone()).or_default();
            tally.checked += 1;
            if actual == expected {
                return;
            }
            match sanction {
                Some(_) => tally.sanctioned += 1,
                None => tally
                    .failures
                    .push(format!("{label}: got {actual:?}, oracle {expected:?}")),
            }
        };
        match kind.as_str() {
            "float_repr" => {
                let bits = record["bits"].as_u64().unwrap();
                check(
                    format!("float_repr of bits {bits:#018x}"),
                    primitives::float_repr(f64::from_bits(bits)),
                    text_of("expected").unwrap(),
                );
            }
            "py_round" => {
                let bits = record["bits"].as_u64().unwrap();
                let actual = primitives::rounded_copper(f64::from_bits(bits))
                    .map(|c| c.0.to_string())
                    .unwrap_or_else(|e| format!("error: {e}"));
                check(
                    format!("round({})", f64::from_bits(bits)),
                    actual,
                    record["expected"].to_string(),
                );
            }
            "money" => {
                let value = &record["value"];
                // Python keeps the sign of a negative value that rounds to zero.
                let expected = match text_of("expected").unwrap().as_str() {
                    "-0c" => "0c".to_owned(),
                    other => other.to_owned(),
                };
                let actual = if value.is_null() || value.is_i64() {
                    primitives::copper_money(value.as_i64().map(Copper))
                } else {
                    primitives::rounded_copper(value.as_f64().unwrap())
                        .map(|copper| primitives::copper_money(Some(copper)))
                        .unwrap_or_else(|e| format!("error: {e}"))
                };
                check(format!("money({value})"), actual, expected);
            }
            "human" => {
                let value = &record["value"];
                check(
                    format!("human({value})"),
                    primitives::human(
                        serde_json::from_value::<Option<primitives::Scalar>>(value.clone())
                            .unwrap()
                            .as_ref(),
                    ),
                    text_of("expected").unwrap(),
                );
            }
            "casefold_cp" => {
                let cp = record["cp"].as_u64().unwrap() as u32;
                let c = char::from_u32(cp).unwrap().to_string();
                check(
                    format!("casefold of U+{cp:04X}"),
                    primitives::casefold(&c),
                    text_of("casefold").unwrap(),
                );
                check(
                    format!("lower of U+{cp:04X}"),
                    primitives::lower(&c),
                    text_of("lower").unwrap(),
                );
            }
            "casefold" => {
                let input = text_of("input").unwrap();
                check(
                    format!("casefold({input:?})"),
                    primitives::casefold(&input),
                    text_of("casefold").unwrap(),
                );
                check(
                    format!("lower({input:?})"),
                    primitives::lower(&input),
                    text_of("lower").unwrap(),
                );
            }
            "quote" => {
                let input = text_of("input").unwrap();
                check(
                    format!("quote({input:?})"),
                    primitives::quote(&input),
                    text_of("expected").unwrap(),
                );
            }
            "urlencode" => {
                let params = record["dedup"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pair| (pair[0].as_str().unwrap().to_owned(), document(&pair[1])))
                    .collect::<Vec<_>>();
                check(
                    format!("urlencode({})", record["dedup"]),
                    primitives::urlencode(&params).unwrap(),
                    text_of("expected").unwrap(),
                );
            }
            "b64_encode" => {
                let encoded = text_of("hex").unwrap();
                let bytes = (0..encoded.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
                    .collect::<Vec<_>>();
                check(
                    format!("b64encode({encoded})"),
                    primitives::base64_encode(&bytes),
                    text_of("expected").unwrap(),
                );
            }
            "b64_decode" => {
                let input = text_of("input").unwrap();
                let actual = primitives::base64_decode(&input)
                    .map_or("error".into(), |bytes| format!("ok {}", hex(&bytes)));
                let expected = if record["ok"].as_bool().unwrap() {
                    format!("ok {}", text_of("hex").unwrap())
                } else {
                    "error".into()
                };
                check(format!("b64decode({input:?})"), actual, expected);
            }
            "json" => {
                let input = text_of("input").unwrap();
                let short = input.chars().take(70).collect::<String>();
                match (parse(&input), record["ok"].as_bool().unwrap()) {
                    (Ok(doc), true) => {
                        for (mode, key) in [
                            (JsonMode::Stdout, "plain"),
                            (JsonMode::Error, "ascii"),
                            (JsonMode::Cursor, "compact_ascii"),
                            (JsonMode::Catalogue, "indent1"),
                        ] {
                            check(
                                format!("json {key} of {short:?}"),
                                dumps(&doc, mode),
                                text_of(key).unwrap(),
                            );
                        }
                    }
                    (Err(_), false) => check(
                        format!("json rejects {short:?}"),
                        "reject".into(),
                        "reject".into(),
                    ),
                    (Ok(doc), false) => check(
                        format!("json {short:?}"),
                        format!("accepted as {}", dumps(&doc, JsonMode::Stdout)),
                        "reject".into(),
                    ),
                    (Err(e), true) => check(
                        format!("json {short:?}"),
                        format!("rejected ({e})"),
                        format!("accept as {}", text_of("plain").unwrap()),
                    ),
                }
            }
            "instant" => {
                let (zone, input) = (text_of("tz").unwrap(), text_of("input").unwrap());
                let actual = time::instant_in(&input, Some(&zone)).map_or("error".into(), |t| {
                    format!("{} / {}", time::iso(t), t.microseconds())
                });
                let expected = if record["ok"].as_bool().unwrap() {
                    format!("{} / {}", text_of("iso").unwrap(), record["micros"])
                } else {
                    "error".into()
                };
                check(format!("instant({input:?}) in {zone}"), actual, expected);
            }
            "time_selector" => {
                let (zone, input) = (text_of("tz").unwrap(), text_of("input").unwrap());
                let actual = match time::time_selector_in(&input, &clock, Some(&zone)) {
                    Ok(text) => text,
                    Err(e) => format!("failure {} {}", e.code as i32, e.message),
                };
                let expected = if record["ok"].as_bool().unwrap() {
                    text_of("expected").unwrap()
                } else {
                    format!("failure {} {}", record["code"], text_of("message").unwrap())
                };
                check(
                    format!("time_selector({input:?}) in {zone}"),
                    actual,
                    expected,
                );
            }
            "age" => {
                let zone = text_of("tz").unwrap();
                let input = text_of("input");
                let actual = time::age_in(input.as_deref(), &clock, Some(&zone));
                check(
                    format!("age({input:?}) in {zone}"),
                    actual.map_or("null".into(), |n| n.to_string()),
                    record["expected"].to_string(),
                );
            }
            "roster_seconds" => {
                let value = &record["value"];
                let actual = time::roster_seconds_in(&document(value), Some("UTC"))
                    .map_or("error".into(), |seconds| seconds.0.to_string());
                let expected = if record["ok"].as_bool().unwrap() {
                    record["expected"].to_string()
                } else {
                    "error".into()
                };
                check(format!("roster_seconds({value})"), actual, expected);
            }
            "iso" => {
                let micros = record["micros"].as_i64().unwrap();
                let instant = time::UtcInstant::from_microseconds(micros).unwrap();
                check(
                    format!("iso({micros})"),
                    time::iso(instant),
                    text_of("expected").unwrap(),
                );
            }
            other => panic!("unknown probe kind {other}"),
        }
    }

    let limit = if std::env::var_os("EVERLEDGER_PROBE_ALL").is_some() {
        usize::MAX
    } else {
        10
    };
    let mut failed = 0;
    for (kind, tally) in &tallies {
        println!(
            "{kind:15} checked {:6}  sanctioned {:3}  failed {:5}",
            tally.checked,
            tally.sanctioned,
            tally.failures.len()
        );
        for failure in tally.failures.iter().take(limit) {
            println!("    {failure}");
        }
        failed += tally.failures.len();
    }
    for (input, reason) in SANCTIONED_TIME_INPUTS {
        println!("sanctioned {input:?}: {reason}");
    }
    println!("{unrepresentable} records skipped: unpaired surrogates cannot be Rust strings");
    assert_eq!(failed, 0, "{failed} probe cases differ from the oracle");
}
