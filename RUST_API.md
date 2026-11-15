# Rust foundation API

Rust 1.100.0, edition 2024, is pinned in `rust-toolchain.toml`. The library and
thin executable forbid unsafe code. `cargo build` produces
`target/debug/everledger`. All sixteen oracle commands are registered;
`bridge` and `verify` are implemented. Other commands return a named
`Failure` until their owner implements them.

## Command ownership and typed arguments

A command owns `src/commands/<command>.rs`, its human rendering in
`src/human/<command>.rs`, and its tests. Both modules are already registered.
Implementation needs no edits to registration or the manifest. Tests may live
inside the command module or in automatically discovered `tests/<command>.rs`.
`parity/` and the independent probe corpus are reviewer-owned.

Every command declares its own argument struct with `#[derive(clap::Args)]`:

```rust
#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true)]
    pub profession: String,
    #[arg(value_parser = crate::cli::parse_integer, allow_negative_numbers = true)]
    pub start: i64,
    #[arg(value_parser = crate::cli::parse_integer, allow_negative_numbers = true)]
    pub end: i64,
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput>;
```

`cli::Command` is a derived subcommand enum holding these structs. Dispatch
matches it exhaustively. Commands never receive `ArgMatches`. Venue values use
`types::Venue::{Auction,Vendor}`; the refresh target uses
`commands::refresh::Target::Catalogue`.

| API | Signature |
| --- | --- |
| `cli::parser` | `() -> clap::Command`, for help/definition inspection |
| `cli::parse_env` | `() -> Result<cli::Command, clap::Error>` |
| `cli::parse_from` | `(impl IntoIterator<Item=impl Into<OsString>>) -> Result<Command, clap::Error>` |
| `cli::parse_integer` | `(&str) -> Result<i64, String>` |
| `cli::dispatch` | `(&Command, &Context) -> Result<CommandOutput>` |
| `cli::execute` | `(&Command, &Context, &mut dyn Write) -> ExitCode` |
| `cli::write_failure` | `(&Failure, &dyn Diagnostics) -> Result<()>` |

Use `parse_from` for fixture invocations. It applies argparse's contiguous
variadic positionals and separator rules as well as the derived schema. Help
subcommands and abbreviated flags are rejected. Numeric arguments accept ASCII
digits, a sign, internal underscores and surrounding whitespace, within `i64`.
Non-ASCII digits and overflowing integers are sanctioned usage errors.
Repeated options keep the last value. `--json` is subcommand-local. The events
command owns cursor-dependent limits and semantic checks; clap leaves its limit
as `Option<i64>`.

`CommandOutput::new(data: Document, lines: Vec<String>) -> CommandOutput` defaults
to exit 0, inferred stale status and no source families. Its fields are `data`,
`lines`, `code: ExitCode`, `stale: Option<bool>`, and
`source_families: human::SourceFamilies`. All human lines originate in a human
module, using typed command/report values.

## Collaborators and immediate diagnostics

```rust
pub struct Context<'a> {
    pub clock: &'a dyn clock::Clock,
    pub http: &'a dyn http::Http,
    pub config: &'a config::Config,
    pub catalogue: &'a catalogue::CatalogueStore,
    pub diagnostics: &'a dyn diagnostics::Diagnostics,
}
```

`diagnostics::Diagnostics` exposes `write(&self, &str) -> Result<()>` and
`warn(&self, &str) -> Result<()>`. `warn` appends a newline and writes immediately:

```rust
context.diagnostics.warn("deprecated --known; use --char")?;
return Err(Failure::usage("--known conflicts with --char"));
```

The warning survives the failure. Resync notices, cache-retention notices,
deprecations and catalogue-age advisories use the same collaborator. No command
needs to capture process stderr or carry warnings out of a successful return.

`WriterDiagnostics<W: Write>::new(W)` adapts a writer. `into_inner(self) -> W`
returns it. `WriterDiagnostics<Vec<u8>>::default()` records fixture diagnostics;
`contents(&self) -> Vec<u8>` reads them. The executable injects standard stderr.

For an in-process command test, create a virtual clock, a fixture `Http`,
`Config::from_values`, a `CatalogueStore::from_document`, and recording
`WriterDiagnostics`. Construct `Context`, parse argv with `parse_from`, and call
`execute` with a `Vec<u8>` stdout writer. Assert the exit code, stdout,
diagnostics, and fixture HTTP transcript. A fresh catalogue store isolates each
fixture; a reused store intentionally caches successful loads.

## Units, failures and configuration

| Path | Representation / signature |
| --- | --- |
| `types::Copper` | `pub struct Copper(pub i64)`, serde integer, ordered |
| `types::EpochSeconds` | `pub struct EpochSeconds(pub i64)`, serde integer, ordered |
| `time::UtcInstant` | private checked UTC microseconds, ordered |
| `types::ExitCode` | `Answer=0`, `Error=1`, `Usage=2`, `Stale=3`, `Unreachable=4` |
| `types::Failure` | `{message: String, code: ExitCode, body: Option<Document>}` |
| `types::Result<T>` | `Result<T, Failure>` |
| `Failure::new` | `(impl Into<String>, ExitCode) -> Failure` |
| `Failure::{error,usage,unreachable}` | `(impl Into<String>) -> Failure` |
| `Failure::shape` | `(&str, impl Display) -> Failure`, exit 1 |
| `config::Config::from_env` | `() -> Config`, tolerates non-Unicode environment values |
| `Config::from_values` | `(&HashMap<String,String>) -> Config` |
| `Config::roster_token` | `(&self) -> Result<String>`, environment/file fallback |
| `Config::foreverdb_base` | `(&self) -> Result<&str>`, seam-aware |
| `config::FOREVERDB_DAILY_REQUESTS` | `const u32 = 36`, no override |

`Config` exposes data directory, roster/AHledger/vendor URLs, market, roster
token and file, ForeverDB key and file, connector slug and dynamic-credentials
directory. Empty token-file settings are unset. `foreverdb_url: Option<String>`
exists only under `parity-seams`. Proxy and CA variables are read by HTTP.

## Documents and consistent typed views

`document::Document` is `Null | Bool(bool) | Number(Number) | String(String) |
Array(Vec<Document>) | Object(Vec<(String,Document)>)`. `Number` is
`Integer(String) | Float(f64)`. JSON integers retain arbitrary precision;
unit-bearing typed fields remain bounded. Object updates preserve first insertion
positions and replace duplicate values. Equality ignores object key order and
uses Python numeric equality.

| API | Signature |
| --- | --- |
| `document::parse` | `(&str) -> Result<Document>` |
| `document::dumps` | `(&Document, JsonMode) -> String`, no trailing newline |
| `Document::object` | `(impl IntoIterator<Item=(impl Into<String>,Document)>) -> Document` |
| `Document::get` | `(&self, &str) -> Option<&Document>` |
| `Document::{set,set_default}` | `(&mut self, impl Into<String>, impl Into<Document>) -> Result<()>` |
| `Document::remove` | `(&mut self, &str) -> Option<Document>` |
| `Document::decode<T: DeserializeOwned>` | `(&self, boundary: &str) -> Result<T>` |
| `Document::from_typed<T: Serialize>` | `(&T) -> Result<Document>` |
| `Document::{as_str,as_array,as_object,as_f64,as_i64}` | optional typed accessors |
| `Document::truthy` | `(&self) -> bool` |
| `http::Payload<T>::decode` | `(Document, boundary: &str) -> Result<Payload<T>>` |
| `Payload::document` | `(&self) -> &Document` |
| `Payload::view` | `(&self) -> &T` |
| `Payload::{set,set_default}` | `(&mut self, impl Into<String>, impl Into<Document>) -> Result<()>` |
| `Payload::into_document` | `(self) -> Document` |

Payload fields are private. Updates clone the ordered document, decode the new
typed view and commit both together. Invalid updates leave both unchanged. There
is no mutable view or mutable document accessor. Use typed views for decisions
and retained documents for pass-through output. Required fields are required
serde fields; optional `.get` fields use `Option<T>`. `provenance::Field<T>`
(`Missing | Null | Value(T)`) distinguishes absence from an explicit null;
`as_ref()` borrows its value. `catalogue::nullable` supplies required nullable
serde fields through `deserialize_with`.

`JsonMode::Stdout` also serves cache writes: Unicode, spaced separators, no
indent. Add a newline only for stdout. `Error` escapes ASCII-external characters,
including DEL, with spaced separators. `Cursor` uses ASCII escaping and compact
separators. `Catalogue` uses Unicode and one-space indentation. The parser checks
number grammar itself and rejects excessive nesting with Failure. serde_json
uses `float_roundtrip`; untagged enums and flatten work without
`arbitrary_precision`.

## Scalar, numeric and encoding primitives

| API | Signature |
| --- | --- |
| `primitives::float_repr` | `(f64) -> String`, Python shortest digits/ties and exponent text |
| `primitives::rounded_copper` | `(f64) -> Result<Copper>`, half-even integral rounding |
| `primitives::copper_money` | `(Option<Copper>) -> String`, exact money text |
| `primitives::Scalar` | serde untagged `Bool(bool) | Integer(i64) | Float(f64) | Text(String)` |
| `Scalar::python_str` | `(&self) -> String` |
| `primitives::human` | `(Option<&Scalar>) -> String` |
| `primitives::py_str` | `(&Document) -> Result<String>`, scalar boundary conversion |
| `primitives::casefold` | `(&str) -> String`, Python Unicode 15.1 table |
| `primitives::lower` | `(&str) -> String`, Rust string lowercase |
| `primitives::quote` | `(&str) -> String`, UTF-8 escaping with `safe=''` |
| `primitives::urlencode` | `(&[(String,Document)]) -> Result<String>`, ordered scalar parameters |
| `primitives::base64_encode` | `(&[u8]) -> String`, padded URL-safe base64 |
| `primitives::base64_decode` | `(&str) -> Result<Vec<u8>,Base64Error>` |

`Base64Error::{InvalidAlphabet,InvalidPadding}` names decoding failures; page
cursor code maps them to its own `Failure`. Money formats typed copper only;
a negative input rounding to zero prints `0c`. Containers in scalar positions
are shape drift. Lowercase uses Rust's contextual mapping; mappings for the
75 newer Unicode letters are sanctioned differences.

Typed human helpers live in `human`: `text(Option<&str>) -> String`,
`boolean(Option<bool>) -> &'static str`, `integer(Option<i64>) -> String`,
`number(Option<f64>) -> String`, `value(Option<&Scalar>) -> String`, and re-exported
`copper_money`. Unknown values print `unknown`; booleans print `yes`/`no`.

## Time and clock APIs

| API | Signature |
| --- | --- |
| `UtcInstant::from_microseconds` | `(i64) -> Result<UtcInstant>` |
| `UtcInstant::from_epoch` | `(EpochSeconds) -> Result<UtcInstant>` |
| `UtcInstant::microseconds` | `(self) -> i64` |
| `UtcInstant::add_microseconds` | `(self, i64) -> Result<UtcInstant>` |
| `clock::Clock::{now,monotonic}` | `(&self) -> UtcInstant`, `(&self) -> f64` |
| `Clock::sleep` | `(&self, f64) -> Result<()>` |
| `clock::SystemClock::default` | `() -> SystemClock` |
| `clock::VirtualClock::new` | `(UtcInstant) -> VirtualClock` |
| `VirtualClock::from_env` | `() -> Result<Option<VirtualClock>>`, feature only |
| `time::iso` | `(UtcInstant) -> String`, UTC seconds and `Z` |
| `time::{instant,instant_in}` | `(&str[, zone: Option<&str>]) -> Result<UtcInstant>` |
| `time::{time_selector,time_selector_in}` | `(&str, &dyn Clock[, Option<&str>]) -> Result<String>` |
| `time::{roster_seconds,roster_seconds_in}` | `(&Document[, Option<&str>]) -> Result<EpochSeconds>` |
| `time::{age,age_in}` | `(Option<&str>, &dyn Clock[, Option<&str>]) -> Option<i64>` |

Instants span `0001-01-01T00:00:00Z` through
`9999-12-31T23:59:59.999999Z`; constructors and arithmetic reject overflow.
Jiff civil values and timezone offsets support the complete range, including
Python's last day beyond jiff's absolute Timestamp range. Naive inputs use `TZ`
or the system timezone, with Python's default fold/gap offsets. Fractions truncate
to microseconds. The three sanctioned CPython artifacts are recorded in the
independent probe. Relative selectors and roster epoch strings read ASCII digits,
like CLI integer arguments; non-ASCII digits are a sanctioned difference.

The virtual clock stores one f64 elapsed value. Each sleep validates its new
wall time before committing state or logging. Monotonic readings use elapsed;
wall readings use origin plus elapsed rounded half-even to microseconds.

## Shared boundaries and rendering

| API | Signature |
| --- | --- |
| `http::Request::get` | `(impl Into<String>) -> Request` |
| `Request::header` | `(self, impl Into<String>, impl Into<String>) -> Self` |
| `http::Http::request` | `(&self, &Request) -> Result<Response>` |
| `http::Response::header` | `(&self, &str) -> Option<&str>`, case-insensitive |
| `http::decode_body` | `(&[u8]) -> Result<Document>`, UTF-8, optional BOM |
| `roster::character_path` | `(&str) -> String` |
| `roster::roster<T: DeserializeOwned>` | `(&dyn Http, &Config, &str, &[(String,Document)]) -> Result<Payload<T>>` |
| `catalogue::CatalogueStore::default` | `() -> CatalogueStore` |
| `CatalogueStore::from_document` | `(Document) -> Result<CatalogueStore>` |
| `CatalogueStore::load` | `(&self, &Config, &dyn Clock, &dyn Diagnostics) -> Result<&Catalogue>` |
| `catalogue::load_catalogue` | `(&Context) -> Result<&Catalogue>` |
| `catalogue::catalogue_source` | `(&Catalogue, &dyn Clock) -> Result<Source>` |
| `catalogue::recipes` | `(&Catalogue) -> Result<Vec<RecipeRef<'_>>>` |
| `catalogue::resolve_item` | `(&str, &Catalogue) -> Result<Item>` |
| `catalogue::item_for` | `(&str, &Context) -> Result<Item>` |
| `catalogue::item_matches` | `(&ItemRow, &Item) -> bool` |
| `catalogue::item_basis` | `(&ItemRow) -> MatchBasis`, `Id | Name` |
| `catalogue::disclose_item_source` | `(&mut Document, Option<&Item>, &Context) -> Result<()>` |
| `market::market_table` | `(&Context) -> Result<(PriceTable,Source)>` |
| `provenance::source` | `(&str, Option<&str>, &dyn Clock) -> Result<Source>` |
| `provenance::sources` | `(&Document) -> Result<Vec<Source>>` |
| `provenance::stale_code` | `(&[Source]) -> ExitCode` |
| `human::source_lines` | `(&[Source], &SourceFamilies, &dyn Clock) -> String` |
| `human::emit` | `(&Document, &[String], bool, Option<bool>, &SourceFamilies, &dyn Clock) -> Result<String>` |

`Response` contains public body bytes and headers. `Request.redirects` is
`Redirects::{Follow,Refuse}`. `UreqHttp` supplies the oracle headers and 20-second
connect/read/write timeouts. HTTP 400/422 map to 2; 404/409 to 1; other HTTP and
network failures to 4. Failure bodies retain status and decoded JSON response.
Error messages name the originally requested URL after a redirect.

`Catalogue = Payload<CatalogueData>`. Its view holds `generated_at`,
`Field<String>` source and `OrderedMap<Profession>`. Ordered map entries are in
`.0`; `.get(&str)` finds a key. `RecipeRef` borrows profession, typed recipe and
original recipe document. Bands have integral learn and nullable thresholds;
recipes carry result, reagents and nullable source type. `Item` holds an integral
ID and optional name. `ItemRow` and `RecordedItem` preserve missing/null fallback
semantics for matching.

`PriceTable = BTreeMap<i64,Price>`; prices contain optional median/floor `Copper`
and quantity/sales floats. `Source = Payload<SourceData>` retains full original
provenance. Its view contains names, paths, timestamps, detail, stale flag and
`Field<SourceAge>` ages. `SourceAge::{Integer,Float}` preserves number class;
`.seconds()` compares values and `.text()` formats them.

`SourceFamilies = Vec<(String,Vec<Source>)>`. Membership uses document value
equality. `source_lines` returns one footer without a newline. `emit` returns
complete stdout with a trailing newline; it prints one footer after rows and any
stale notice. Inferred staleness excludes Journalator sources.

## Dependencies, seams and verification

Direct dependencies are exactly pinned: clap 4.6.7, serde 1.0.229, serde_json
1.0.154, ureq 3.5.2 and jiff 0.2.45. Clap derives keep argument types and parsing
in one declaration. Jiff supplies civil dates, checked arithmetic and local-zone
offsets; the independent oracle probe verifies the Python-compatible parser.

Ureq uses rustls with `RootCerts::PlatformVerifier`. The forward-proxy transport
adapter implements urllib's absolute-form HTTP requests; HTTPS uses ureq's
standard CONNECT/TLS path. Loopback tests cover scheme-specific proxies,
`no_proxy`, redirects, headers and status mapping. A local CA supplied through
`SSL_CERT_FILE` works for direct TLS and authenticated CONNECT requests. No
additional direct crate is needed.

`parity-seams` enables the shared virtual clock origin/sleep log and the
ForeverDB URL override. Both configuration and HTTP fail closed for ForeverDB
without that override. Default release builds contain no seam environment
handling. The daily cap is a compile-time constant.

`python3 scripts/gen_python_vectors.py` regenerates retained golden vectors and
the casefold table. `scripts/gen_oracle_probe.py` generates the independent
reviewer-owned corpus. `cargo test --offline --all-features` consumes committed
vectors with no Python interpreter or external network. Tests use only loopback
servers or injected fixtures. Run the reviewer argv surface check and selected
differential scenarios against a binary built with `parity-seams`, then
`cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`.
