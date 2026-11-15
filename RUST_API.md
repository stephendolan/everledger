# Rust foundation API

The crate uses Rust 1.100.0, edition 2024, and forbids unsafe code. `cargo build`
produces `target/debug/everledger`. The executable parses arguments, constructs
collaborators, calls the library, and returns its exit code.

## Command ownership and implementation

All sixteen oracle commands are registered. `verify` and `bridge` run end to end;
the other fourteen return `<command>: not yet ported` with exit 1.

Each command owns `src/commands/<command>.rs`, its rendering module
`src/human/<command>.rs`, and its tests. All rendering modules are already declared.
Implementing a command needs no changes to registration, shared modules, or the
manifest. Command-specific boundary structs and serde decoding belong in that
command's files. New command integration tests can live in `tests/<command>.rs`,
which Cargo discovers automatically beside the existing Python tests. Unit tests
can live in the command module itself. `parity/` belongs to the harness worker.

```rust
pub fn run(args: &clap::ArgMatches, context: &everledger::Context<'_>)
    -> everledger::Result<everledger::commands::CommandOutput>;

pub struct Context<'a> {
    pub clock: &'a dyn clock::Clock,
    pub http: &'a dyn http::Http,
    pub config: &'a config::Config,
    pub catalogue: &'a catalogue::CatalogueStore,
}
```

`commands::CommandOutput::new(data: Document, lines: Vec<String>) -> Self` defaults
to exit 0, no warnings, inferred staleness, and no source families. Its public
fields are `data`, `lines`, `code: ExitCode`, `warnings: Vec<String>`,
`stale: Option<bool>`, and `source_families: human::SourceFamilies`. Produce all
human strings in the command's human module from typed values. The CLI owns the
shared emit/error paths. It prints verify's deprecation warning before loading
its catalogue, including on failure.

Numeric CLI options and bridge endpoints use `types::CliInteger`, an ordered,
arbitrary precision integer with Python decimal-digit, sign, whitespace and
underscore parsing. Read them with `args.get_one::<CliInteger>("limit")`.
`as_i64() -> Option<i64>` supports bounded calculations; `document() -> Document`
and `Display` preserve arbitrary magnitudes. `is_negative() -> bool` supports
usage checks. `From<i64>`, `FromStr`, `Ord`, and `Eq` are implemented. No default
limit is injected by clap: the events command owns the human/JSON defaults and
cursor-dependent omission, as in Python. The CLI already checks empty cursor
and negative limit before dispatch. Other commands' semantic validation belongs
in their command files.

## Units, failures and configuration

| Path | Signature / representation |
| --- | --- |
| `types::Copper` | `pub struct Copper(pub i64)`, serde integer, ordered |
| `types::EpochSeconds` | `pub struct EpochSeconds(pub i64)`, serde integer, ordered |
| `time::UtcInstant` | `pub struct UtcInstant(pub jiff::Timestamp)`, microsecond precision |
| `types::ExitCode` | `Answer=0`, `Error=1`, `Usage=2`, `Stale=3`, `Unreachable=4` |
| `types::Failure` | `{ message: String, code: ExitCode, body: Option<Document> }` |
| `types::Result<T>` | `Result<T, Failure>` |
| `Failure::new` | `(message: impl Into<String>, code: ExitCode) -> Failure` |
| `Failure::{error,usage,unreachable}` | `(message: impl Into<String>) -> Failure` |
| `Failure::shape` | `(boundary: &str, error: impl Display) -> Failure`, exit 1 |
| `config::Config::from_env` | `() -> Config` |
| `config::Config::from_values` | `(&HashMap<String, String>) -> Config`, fixture-friendly |
| `Config::roster_token` | `(&self) -> Result<String>`, trimmed environment/file fallback |
| `Config::foreverdb_base` | `(&self) -> Result<&str>`, seam-aware and fail-closed |
| `config::FOREVERDB_DAILY_REQUESTS` | `const u32 = 36`, no environment override |

`Config` has public fields for data directory, roster/AHledger/vendor URLs,
market, roster token and file, ForeverDB key and file, connector slug and dynamic
credentials directory. Defaults and empty-value behavior follow the oracle.
`foreverdb_url: Option<String>` exists only under `parity-seams`. Proxy and CA
environment variables are read by the HTTP transport at request time.

## Ordered JSON and Python primitives

`document::Document` is `Null | Bool(bool) | Number(Number) | String(String) |
Array(Vec<Document>) | Object(Vec<(String, Document)>)`. `Number` is
`Integer(String) | Float(f64)`. Integer text is unbounded. Objects preserve first
key insertion positions and replace duplicate values. Value equality follows
Python object equality, independent of key order, including bool/int equality.

| Path | Signature |
| --- | --- |
| `document::parse` | `(&str) -> Result<Document>` |
| `document::dumps` | `(&Document, JsonMode) -> String`, no trailing newline |
| `document::JsonMode` | `Stdout`, `Error`, `Cursor`, `Catalogue` |
| `Document::object` | `(impl IntoIterator<Item=(impl Into<String>, Document)>) -> Document` |
| `Document::get` | `(&self, key: &str) -> Option<&Document>` |
| `Document::set` | `(&mut self, key: impl Into<String>, value: impl Into<Document>)` |
| `Document::set_default` | same arguments; leaves an existing null intact |
| `Document::remove` | `(&mut self, key: &str) -> Option<Document>` |
| `Document::decode<T: DeserializeOwned>` | `(&self, boundary: &str) -> Result<T>` |
| `Document::from_typed<T: Serialize>` | `(&T) -> Result<Document>` |
| `Document::{as_str,as_array,as_object,as_f64,as_i64}` | optional typed accessors |
| `Document::truthy` | `(&self) -> bool`, Python truthiness |
| `primitives::float_repr` | `(f64) -> String`, Python shortest text and ties |
| `primitives::round` | `(f64, digits: i32) -> f64`, half-even decimal rounding |
| `primitives::rounded_copper` | `(f64) -> Result<Copper>`, integral half-even rounding |
| `primitives::py_str` | `(&Document) -> String`, Python strings/numbers/containers |
| `primitives::human` | `(&Document) -> String`, unknown/yes/no conventions |
| `primitives::money` | `(Option<f64>) -> Result<String>`, boundary numeric formatting |
| `primitives::copper_money` | `(Option<Copper>) -> String`, exact typed command formatting |
| `primitives::{casefold,lower}` | `(&str) -> String`, Python Unicode 15.1 mappings/context |
| `primitives::quote` | `(&str) -> String`, UTF-8 percent escaping, `safe=''` |
| `primitives::urlencode` | `(&[(String, Document)]) -> String`, ordered, space becomes `+` |
| `primitives::base64_encode` | `(&[u8]) -> String`, URL-safe with padding |
| `primitives::base64_decode` | `(&str) -> Result<Vec<u8>>`, strict Python 3.13 padding |
| `primitives::decimal` | `(&str) -> Option<String>`, Unicode decimal normalization |
| `primitives::isdigit` | `(&str) -> bool`, Python digit classification |

`Stdout` also serves cache writes: Unicode, comma-space, colon-space, no indent.
Add a newline only for stdout. `Error` uses ASCII escaping and spaced separators;
`Cursor` uses ASCII escaping and compact separators; `Catalogue` uses Unicode and
one-space indentation. Avoid serde_json output and float `Display` for wire text.
Serde is for typed boundary views; retain `Document` for pass-through output.
`Document::from_typed` is for new, typed output; it does not recover the original
field order or numeric class of a received payload.

## Time and clocks

| Path | Signature |
| --- | --- |
| `clock::Clock::now` | `(&self) -> UtcInstant` |
| `clock::Clock::monotonic` | `(&self) -> f64`, elapsed seconds |
| `clock::Clock::sleep` | `(&self, seconds: f64) -> Result<()>` |
| `clock::SystemClock::default` | `() -> SystemClock` |
| `clock::VirtualClock::new` | `(origin: UtcInstant) -> VirtualClock` |
| `clock::VirtualClock::from_env` | `() -> Result<Option<VirtualClock>>`, feature only |
| `time::iso` | `(UtcInstant) -> String`, UTC seconds with `Z` |
| `time::instant` | `(&str) -> Result<UtcInstant>`, Python ISO parsing/local TZ |
| `time::instant_in` | `(&str, zone: Option<&str>) -> Result<UtcInstant>` |
| `time::time_selector` | `(&str, &dyn Clock) -> Result<String>` |
| `time::time_selector_in` | `(&str, &dyn Clock, Option<&str>) -> Result<String>` |
| `time::roster_seconds` | `(&Document) -> Result<EpochSeconds>`, bool before string |
| `time::roster_seconds_in` | `(&Document, Option<&str>) -> Result<EpochSeconds>` |
| `time::age` | `(Option<&str>, &dyn Clock) -> Option<i64>` |
| `time::age_in` | `(Option<&str>, &dyn Clock, Option<&str>) -> Option<i64>` |
| `UtcInstant::from_epoch` | `(EpochSeconds) -> Result<UtcInstant>` |
| `UtcInstant::microseconds` | `(self) -> i64`, use for duration arithmetic |
| `UtcInstant::add_microseconds` | `(self, i64) -> Result<UtcInstant>` |

The `_in` helpers supply an explicit zone for fixture tests. Otherwise naive
inputs use the local zone, including `TZ`. Parsing handles basic and extended
calendar/week dates, arbitrary date/time separators, fractional seconds and
fractional offsets, and truncation to six decimal places. Jiff's compatible
ambiguity resolution matches Python's default fold and gap behavior. The virtual
clock shares one f64 elapsed value across monotonic readings, microsecond wall
time, and sleep logging. Sleeps advance it immediately. Origin and sleep-log
variables are read only when `parity-seams` is compiled.

## HTTP and typed boundaries

```rust
http::Request::get(url: impl Into<String>) -> Request;
Request::header(self, name: impl Into<String>, value: impl Into<String>) -> Self;
http::Http::request(&self, request: &Request) -> Result<http::Response>;
http::Response { body: Vec<u8>, headers: Vec<(String, String)> }
Response::header(&self, name: &str) -> Option<&str>; // case-insensitive
http::decode_body(&[u8]) -> Result<Document>;
http::Payload<T> { document: Document, view: T }
Payload::<T>::decode(document: Document, boundary: &str) -> Result<Payload<T>>;
```

`Request.redirects` is `Redirects::Follow` or `Redirects::Refuse`. `UreqHttp`
implements `Http`. It supplies the oracle User-Agent, identity encoding and
connection close, honors caller header overrides, follows urllib's redirect
limits, and applies 20-second connect/read/write timeouts. HTTP 400/422 map to 2,
404/409 to 1, and other HTTP/network errors to 4. HTTP failures carry
`{"status": status, "response": decoded_body_or_null}`. UTF-8/16/32 JSON bytes
are decoded at the edge. Redirects re-evaluate proxy selection.

The pinned ureq 3.5.2 client uses rustls and `RootCerts::PlatformVerifier`.
The real build and loopback tests establish HTTP forward proxy selection,
`no_proxy`, headers, redirect paths and status mapping. A temporary local CA
installed through `SSL_CERT_FILE` passes both direct TLS and authenticated
CONNECT-proxy requests; the origin receives the expected path. Ureq 3's native
HTTP proxy transport is CONNECT-only. `http_forward.rs` adapts its documented
connector/resolver/transport interfaces for urllib's absolute-form HTTP requests;
ureq still parses all HTTP responses. HTTPS uses ureq's standard CONNECT/TLS
transport. No extra direct crate is needed. Proxy credentials stay in proxy
headers and are not copied to the origin.

Jiff 0.2.45 supplies checked civil dates, ISO weeks, microsecond timestamps and
local-zone ambiguity handling. The Python-compatible parser sits above it
because Jiff's own ISO parser has a different acceptance surface. Committed
vectors test UTC, America/New_York and Asia/Kathmandu, including DST gaps/folds.
This confirms the proposed time crate with a real build.

With the parity feature, both `Config::foreverdb_base` and `UreqHttp` fail closed
for ForeverDB unless the seam URL exists. HTTP also redirects a hard-coded
ForeverDB host to the seam, preventing an accidental real-host request. Normal
release builds have no seam environment-variable handling.

## Shared service APIs

| Path | Signature |
| --- | --- |
| `roster::character_path` | `(&str) -> String` |
| `roster::roster<T: DeserializeOwned>` | `(&dyn Http, &Config, path: &str, params: &[(String, Document)]) -> Result<Payload<T>>` |
| `catalogue::CatalogueStore::default` | `() -> CatalogueStore` |
| `CatalogueStore::from_document` | `(Document) -> Result<CatalogueStore>`, in-process fixture |
| `CatalogueStore::load` | `(&self, &Config, &dyn Clock) -> Result<&Catalogue>` |
| `catalogue::load_catalogue` | `(&Context) -> Result<&Catalogue>` |
| `catalogue::catalogue_source` | `(&Catalogue, &dyn Clock) -> Source` |
| `catalogue::recipes` | `(&Catalogue) -> impl Iterator<Item=RecipeRef<'_>>` |
| `catalogue::resolve_item` | `(query: &str, cat: &Catalogue) -> Result<Item>` |
| `catalogue::item_for` | `(query: &str, &Context) -> Result<Item>` |
| `catalogue::item_matches` | `(&ItemRow, &Item) -> bool` |
| `catalogue::item_basis` | `(&ItemRow) -> MatchBasis`, `Id` or `Name` |
| `catalogue::disclose_item_source` | `(&mut Document, Option<&Item>, &Context) -> Result<()>` |
| `market::market_table` | `(&Context) -> Result<(PriceTable, Source)>` |
| `provenance::source` | `(name: &str, timestamp: Option<&str>, &dyn Clock) -> Source` |
| `provenance::sources` | `(&Document) -> Result<Vec<Source>>` |
| `provenance::stale_code` | `(&[Source]) -> ExitCode` |
| `human::source_lines` | `(&[Source], &SourceFamilies, &dyn Clock) -> String` |
| `human::emit` | `(&Document, &[String], bool, Option<bool>, &SourceFamilies, &dyn Clock) -> Result<String>` |
| `cli::parser` | `() -> clap::Command` |
| `cli::dispatch` | `(&ArgMatches, &Context) -> Result<CommandOutput>` |
| `cli::execute` | `(&ArgMatches, &Context, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode` |
| `cli::write_failure` | `(&Failure, &mut dyn Write) -> Result<()>` |

`Catalogue = Payload<CatalogueData>`; its view holds `generated_at`, nullable or
missing `source`, and `OrderedMap<Profession>`. `OrderedMap<T>.0` exposes ordered
entries and `.get(&str)` finds a key. `RecipeRef` holds `profession`, typed
`recipe`, and original recipe `document`. `Bands` has integral `learn` and
nullable yellow/green/grey; `Recipe` carries spell/name/result/reagents and
nullable source type. `Item` is `{id: i64, name: Option<String>}`.

`RecordedItem` and `ItemRow` are reusable serde views for matching. Recorded IDs
and names use `provenance::Field<T>` (`Missing | Null | Value(T)`) so explicit
null does not activate a missing-key fallback. Use the same field type whenever
Python's `.get(key, fallback)` distinguishes absence from null. Ordinary optional
fields use `Option<T>`. `catalogue::nullable` is a serde `deserialize_with` helper
for required keys whose values may be null.

`PriceTable = BTreeMap<i64, Price>`; `Price` has optional typed median/floor
`Copper`, optional quantity and sales/day floats. `Source = Payload<SourceData>`
retains all original provenance fields. Its typed view holds optional names,
paths, timestamps, detail and stale flag; age fields use `Field<SourceAge>`.
`SourceAge` retains integer/float class; `.seconds()` supports comparisons and
`.text()` supports Python formatting. Update the view and document together when
adding a computed source field.

`SourceFamilies = Vec<(String, Vec<Source>)>`. Family membership uses document
value equality. `source_lines` returns the whole footer without a newline.
`emit` returns the entire stdout string with a trailing newline. Inferred stale
notices exclude Journalator; explicit staleness overrides inference. The footer
appears once after all rows and the optional stale notice.

## Verification

`python3 scripts/gen_python_vectors.py` regenerates committed JSON, float,
rounding, Unicode, URL, base64, time, human/provenance and command golden vectors
using the oracle's Python. It also generates Unicode mapping/property tables.
`python3 scripts/gen_cli_surface.py` regenerates clap registration from argparse.
Run `cargo fmt` after regenerating Rust registration.

`cargo test --offline` needs no Python interpreter or network. Feature coverage
uses `cargo test --offline --all-features`. Boundary tests use loopback servers
and in-process typed fixtures. Golden tests include twenty full verify/bridge
cases, null bands/source types, 1,000 random float candidates, and thousands of
rounding comparisons. `cargo fmt --check` and
`cargo clippy --all-targets --all-features -- -D warnings` are the lint gates.

JSON strings containing unpaired UTF-16 surrogates are rejected at the boundary;
Rust strings contain Unicode scalar values. Typed IDs, copper and epoch seconds
are bounded by i64; ordered JSON integers and CLI integers are unbounded. A
non-finite money value returns Failure rather than Python's internal exception.
