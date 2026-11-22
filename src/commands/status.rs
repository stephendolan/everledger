#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, ExitCode, Result,
    commands::CommandOutput,
    document::Document,
    provenance::{Field, Source, is_stale, sources},
    vendors::status::Backend,
};
use serde::Deserialize;
#[derive(Deserialize)]
pub struct StatusRow {
    pub source: String,
    pub backend: Option<Backend>,
    pub line: Option<String>,
    pub market_source: Option<String>,
    #[serde(default)]
    pub state: Field<String>,
}
#[derive(Deserialize)]
struct ServiceStatus {
    #[serde(default)]
    state: Field<String>,
}
#[derive(Deserialize)]
struct SalesLogStatus {
    line: String,
    market_source: String,
}
enum Probe {
    Roster,
    Outcomes,
    Listings,
    SalesLog,
}
impl Probe {
    fn name(&self) -> &'static str {
        match self {
            Self::Roster => "roster",
            Self::Outcomes => "outcomes",
            Self::Listings => "listings",
            Self::SalesLog => "sales log",
        }
    }
}
fn probe(probe: &Probe, context: &Context<'_>) -> Result<(Document, Vec<Source>, bool)> {
    let (data, stale) = match probe {
        Probe::Roster => {
            let data = crate::roster::roster::<ServiceStatus>(
                context.http,
                context.config,
                "/characters",
                &[],
            )?;
            (data.into_document(), false)
        }
        Probe::Outcomes => {
            let query = crate::journal::EventQuery {
                view: crate::journal::View::default(),
                limit: Some(1),
                query: crate::journal::Query::Window {
                    since: None,
                    until: None,
                    order: crate::journal::WindowOrder::SinceUntil,
                },
            };
            (
                crate::journal::event_page(&query, context)?
                    .data
                    .into_document(),
                false,
            )
        }
        Probe::Listings => {
            let data: crate::commands::listings::Listings =
                crate::roster::roster(context.http, context.config, "/listings", &[])?;
            let stale = is_stale(&crate::commands::listings::listing_sources(&data)?);
            (data.into_document(), stale)
        }
        Probe::SalesLog => {
            let response = crate::roster::roster::<SalesLogStatus>(
                context.http,
                context.config,
                "/saleslog/status",
                &[],
            );
            let mut row = match response {
                Ok(data) => {
                    let SalesLogStatus {
                        line,
                        market_source,
                    } = data.view();
                    if line.is_empty() || market_source.is_empty() {
                        return Err(crate::Failure::shape("sales log status", "missing summary"));
                    }
                    data.into_document()
                }
                Err(error)
                    if error
                        .body
                        .as_ref()
                        .and_then(|body| body.get("status"))
                        .and_then(Document::as_i64)
                        == Some(404) =>
                {
                    return Ok((
                        Document::object([
                            ("source", probe.name().into()),
                            ("state", "unavailable (requires roster 1.6 or later)".into()),
                        ]),
                        vec![],
                        false,
                    ));
                }
                Err(error) => return Err(error),
            };
            let used = sources(&row)?;
            row.set("state", "reachable")?;
            row.set("source", probe.name())?;
            let stale = is_stale(&used);
            return Ok((row, used, stale));
        }
    };
    let view: ServiceStatus = data.decode("service status")?;
    let used = sources(&data)?;
    let state = match view.state {
        Field::Missing => "reachable".into(),
        Field::Null => Document::Null,
        Field::Value(s) => s.into(),
    };
    Ok((
        Document::object([
            ("source", probe.name().into()),
            ("state", state),
            ("sources", source_documents(&used)),
        ]),
        used,
        stale,
    ))
}
fn source_documents(sources: &[Source]) -> Document {
    Document::Array(sources.iter().map(|s| s.document().clone()).collect())
}
fn accumulate(code: &mut ExitCode, next: ExitCode) {
    if next as u8 > *code as u8 {
        *code = next;
    }
}
fn failure(label: &str, error: crate::Failure, rows: &mut Vec<Document>, code: &mut ExitCode) {
    rows.push(Document::object([
        ("source", label.into()),
        ("state", error.message.into()),
    ]));
    accumulate(code, error.code);
}
pub fn run(_args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let mut rows = vec![];
    let mut used = vec![];
    let mut roster_sources = vec![];
    let mut code = ExitCode::Answer;
    let mut stale = false;
    match crate::catalogue::load_catalogue(context) {
        Ok(cat) => {
            let (ok, checks) = crate::catalogue_check::check(cat);
            used.push(crate::catalogue::catalogue_source(cat, context.clock)?);
            rows.push(Document::object([
                ("source", "catalogue".into()),
                ("state", if ok { "ok" } else { "anchor failure" }.into()),
                ("updated_at", cat.view().generated_at.clone().into()),
                (
                    "checks",
                    Document::Array(checks.into_iter().map(Document::from).collect()),
                ),
            ]));
            if !ok {
                accumulate(&mut code, ExitCode::Error);
            }
        }
        Err(e) => failure("catalogue", e, &mut rows, &mut code),
    }
    for probe_kind in [
        Probe::Roster,
        Probe::Outcomes,
        Probe::Listings,
        Probe::SalesLog,
    ] {
        match probe(&probe_kind, context) {
            Ok((row, sources, probe_stale)) => {
                roster_sources.extend(sources.clone());
                used.extend(sources);
                rows.push(row);
                stale |= probe_stale;
            }
            Err(e) => failure(probe_kind.name(), e, &mut rows, &mut code),
        }
    }
    match crate::market::market_table(context) {
        Ok((_, source)) => {
            let mut row = Document::object([
                ("source", "AHledger".into()),
                (
                    "state",
                    if source.view().stale == Some(true) {
                        "stale"
                    } else {
                        "reachable"
                    }
                    .into(),
                ),
            ]);
            if let Some(fields) = source.document().as_object() {
                for (key, value) in fields {
                    row.set(key, value.clone())?;
                }
            }
            stale |= is_stale(std::slice::from_ref(&source));
            used.push(source);
            rows.push(row);
        }
        Err(e) => failure("AHledger", e, &mut rows, &mut code),
    }
    let (mut vendor, source) = crate::vendors::status::backend_status(context)?;
    match vendor.view().backend {
        Backend::Foreverdb => {}
        Backend::Ninja => {
            let state = match context
                .http
                .request(&crate::vendors::ninja_request(&context.config.vendor_url))
            {
                Ok(_) => "reachable".into(),
                Err(e) => {
                    accumulate(&mut code, e.code);
                    e.message
                }
            };
            vendor.set("state", state)?;
        }
    }
    stale |= is_stale(&source);
    used.extend(source);
    rows.push(vendor.document().clone());
    let typed = rows
        .iter()
        .map(|r| r.decode("status row"))
        .collect::<Result<Vec<StatusRow>>>()?;
    let lines = crate::human::status::render(&typed, vendor.view());
    let data = Document::object([
        ("status", Document::Array(rows)),
        ("sources", source_documents(&used)),
    ]);
    let mut output = CommandOutput::new(data, lines);
    output.code = code;
    output.stale = stale;
    output.source_families = vec![("roster service".into(), roster_sources)];
    Ok(output)
}
