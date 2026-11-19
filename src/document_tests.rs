use super::{Document, JsonMode, dumps, parse};
use crate::http::Payload;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, fmt::Debug};

fn compare<T: DeserializeOwned + Serialize + Debug>(input: &str) {
    let document = parse(input).unwrap();
    let text = dumps(&document, JsonMode::Stdout);
    let expected = serde_json::from_str::<T>(&text);
    let actual = document.decode::<T>("test boundary");
    match (actual, expected) {
        (Ok(actual), Ok(expected)) => assert_eq!(
            serde_json::to_string(&actual).unwrap(),
            serde_json::to_string(&expected).unwrap(),
            "{input}"
        ),
        (Err(_), Err(_)) => {}
        (actual, expected) => panic!("{input}: direct {actual:?}, JSON {expected:?}"),
    }
}

#[derive(Debug, Deserialize, Serialize)]
enum Choice {
    Unit,
    Newtype(serde_json::Value),
    Tuple(i64, String),
    Struct { value: i64 },
}
#[derive(Debug, Deserialize, Serialize)]
struct Boundary {
    required: i64,
    optional: Option<String>,
    choice: Option<Choice>,
}

#[test]
fn typed_decoding_preserves_json_boundary_acceptance_and_values() {
    for input in [
        "null",
        "true",
        "false",
        "0",
        "-0",
        "-0.0",
        "1.0",
        "-1",
        "1e-45",
        "9223372036854775807",
        "9223372036854775808",
        "-9223372036854775809",
        "18446744073709551615",
        "18446744073709551616",
        "340282366920938463463374607431768211455",
        "1e309",
        "NaN",
        r#""text""#,
        r#"[]"#,
        r#"[1, "a"]"#,
        r#"{}"#,
        r#"{"required":1,"ignored":{"x":[Infinity]}}"#,
        r#"{"required":1,"ignored":1e309}"#,
        r#"{"required":1,"ignored":9999999999999999999999999999999999999999999999}"#,
        r#"{"required":1,"optional":null,"choice":"Unit"}"#,
        r#"{"required":1,"optional":"a","choice":{"Newtype":[1,true]}}"#,
        r#"{"required":1,"choice":{"Tuple":[1,"a"]}}"#,
        r#"{"required":1,"choice":{"Tuple":[1,"a",2]}}"#,
        r#"{"required":1,"choice":{"Struct":{"value":2}}}"#,
        r#"{"required":1,"choice":{"Struct":[2]}}"#,
        r#"{"required":1,"choice":{"Unit":null}}"#,
        r#"{"required":1,"choice":{"Unit":0}}"#,
        r#"{"required":1,"choice":{}}"#,
        r#"{"required":1,"choice":{"Unit":null,"Newtype":0}}"#,
        r#"{"1":2,"-0":3}"#,
        r#"{"01":1}"#,
        r#"{"true":1}"#,
        r#"{"1.0":1}"#,
        r#"{"1e2":1}"#,
        r#"{" 1":1}"#,
    ] {
        compare::<serde_json::Value>(input);
        compare::<Boundary>(input);
        compare::<Option<i64>>(input);
        compare::<Option<u64>>(input);
        compare::<Option<i128>>(input);
        compare::<Option<u128>>(input);
        compare::<Option<f32>>(input);
        compare::<Option<f64>>(input);
        compare::<BTreeMap<i64, i64>>(input);
        compare::<BTreeMap<bool, i64>>(input);
        compare::<(i64, String)>(input);
    }
    for depth in [125, 126, 127, 128, 129, 250] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        compare::<serde_json::Value>(&nested);
        compare::<Boundary>(&format!(r#"{{"required":1,"ignored":{nested}}}"#));
        compare::<Boundary>(&format!(
            r#"{{"required":1,"choice":{{"Newtype":{nested}}}}}"#
        ));
    }
}

#[test]
fn typed_json_values_match_the_oracle_probe_documents() {
    for line in include_str!("../tests/probe/probe.jsonl").lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record["kind"] == "json" && record["ok"] == true {
            compare::<serde_json::Value>(record["input"].as_str().unwrap());
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    value: i64,
    nested: Nested,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Nested {
    count: i64,
}
#[test]
fn payload_edits_preserve_both_representations_and_order_on_failure() {
    let original = r#"{"value": 1, "nested": {"count": 2}}"#;
    let mut payload = Payload::<View>::decode(parse(original).unwrap(), "view").unwrap();
    for path in [
        &["value"][..],
        &["nested", "count"],
        &["nested", "unknown"],
        &["unknown"],
    ] {
        assert!(payload.set_path(path, Document::Null).is_err());
        assert_eq!(dumps(payload.document(), JsonMode::Stdout), original);
        assert_eq!(payload.view().value, 1);
        assert_eq!(payload.view().nested.count, 2);
    }
    assert!(payload.remove("value").is_err());
    assert_eq!(dumps(payload.document(), JsonMode::Stdout), original);
    payload.remove("absent").unwrap();
    payload.set_path(&["nested", "count"], 3i64).unwrap();
    assert_eq!(payload.view().nested.count, 3);
    assert_eq!(
        dumps(payload.document(), JsonMode::Stdout),
        r#"{"value": 1, "nested": {"count": 3}}"#
    );
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum ReferenceScalar {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
}
#[test]
fn scalar_dispatch_preserves_untagged_variant_selection() {
    for input in [
        "false",
        "true",
        "0",
        "-0",
        "1",
        "-1",
        "-0.0",
        "1.0",
        "1e0",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775809",
        "1e-300",
        "1e300",
        r#""text""#,
        "null",
        "[]",
        "{}",
    ] {
        let actual = serde_json::from_str::<crate::primitives::Scalar>(input);
        let expected = serde_json::from_str::<ReferenceScalar>(input);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(
                    serde_json::to_string(&actual).unwrap(),
                    serde_json::to_string(&expected).unwrap(),
                    "{input}"
                );
                let variant = match actual {
                    crate::primitives::Scalar::Bool(_) => 0,
                    crate::primitives::Scalar::Integer(_) => 1,
                    crate::primitives::Scalar::Float(_) => 2,
                    crate::primitives::Scalar::Text(_) => 3,
                };
                let reference = match expected {
                    ReferenceScalar::Bool(_) => 0,
                    ReferenceScalar::Integer(_) => 1,
                    ReferenceScalar::Float(_) => 2,
                    ReferenceScalar::Text(_) => 3,
                };
                assert_eq!(variant, reference, "{input}");
            }
            (Err(_), Err(_)) => {}
            (actual, expected) => panic!("{input}: {actual:?} vs {expected:?}"),
        }
    }
}
