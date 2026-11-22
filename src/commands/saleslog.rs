use crate::{
    Context, Copper, Result,
    cli::OutputArgs,
    commands::CommandOutput,
    document::Document,
    http::Payload,
    provenance::{is_stale, sources},
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(clap::Args, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Command {
    /// Inspect first-party postings and listing-time scan pairings.
    Listings(Window),
    /// Inspect first-party outcomes, linkage, and sale intervals.
    Outcomes(Window),
    /// Read Analysis A and the accumulation status of Analyses B and C.
    Report(ReportArgs),
}

#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
struct Window {
    /// Limit records to the last N days.
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..=36500))]
    days: Option<i64>,
    /// Character name or unique first name.
    #[arg(long = "char", value_name = "NAME")]
    character: Option<String>,
}

impl Window {
    fn query(&self) -> [(String, Document); 2] {
        [
            ("days".into(), self.days.into()),
            ("char".into(), self.character.as_deref().into()),
        ]
    }
}

#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
struct ReportArgs {
    /// Select one analysis (default: all three).
    #[arg(long, value_enum)]
    analysis: Option<Analysis>,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
enum Analysis {
    A,
    B,
    C,
}

impl Analysis {
    fn selector(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    attribution: Attribution,
}

#[derive(Deserialize)]
struct Attribution {
    text: String,
    url: String,
}

#[derive(Deserialize)]
pub struct Listings {
    pub listings: Vec<Listing>,
}

#[derive(Deserialize)]
pub struct Listing {
    pub listing_id: u64,
    pub character_name: String,
    pub item_name_snapshot: String,
    pub quantity_total: u64,
    pub posted_at_earliest: Option<String>,
    pub posted_at_latest: Option<String>,
    pub posted_at_precision: String,
    pub posted_price_per_unit_copper: Copper,
    pub pairing_status: String,
    pub pairing_scan_ts: Option<String>,
    pub pairing_scan_age_seconds: Option<i64>,
    pub floor_at_pairing_copper: Option<Copper>,
    pub median_at_pairing_copper: Option<Copper>,
    pub price_position_bp: Option<i64>,
}

#[derive(Deserialize)]
pub struct Outcomes {
    pub outcomes: Vec<Outcome>,
}

#[derive(Deserialize)]
pub struct Outcome {
    pub outcome_type: String,
    pub character_name: String,
    pub item_name_snapshot: String,
    pub units_involved: u64,
    pub listing_id: Option<u64>,
    pub linkage_status: String,
    pub collected_at: Option<String>,
    pub sale_interval_start: Option<String>,
    pub sale_interval_end: Option<String>,
    pub sale_interval_source: Option<String>,
    pub gross_proceeds_copper: Option<Copper>,
    pub net_proceeds_copper: Option<Copper>,
    pub proceeds_basis: Option<String>,
    pub purchase_price_copper: Option<Copper>,
    pub pairing_status: String,
    pub pairing_scan_ts: Option<String>,
    pub pairing_scan_age_seconds: Option<i64>,
}

#[derive(Deserialize)]
pub struct Reports {
    pub reports: Vec<Report>,
}

#[derive(Deserialize)]
#[serde(tag = "analysis")]
pub enum Report {
    #[serde(rename = "a")]
    A(Box<AnalysisA>),
    #[serde(rename = "b")]
    B(Accumulation),
    #[serde(rename = "c")]
    C(Accumulation),
}

#[derive(Deserialize)]
pub struct AnalysisA {
    pub state: String,
    pub verdict: String,
    pub reason: String,
    pub scope: String,
    pub proceeds_basis: String,
    pub total_count: u64,
    pub decision_grade_count: u64,
    pub excluded_count: u64,
    pub excluded_by_reason: BTreeMap<String, u64>,
    pub linkage_gate: Ratio,
    pub verification: Verification,
    pub listing_time: Sample,
    pub at_most_one_hour: Sample,
    pub scan_age_sensitive: bool,
    pub floor_proxy: FloorProxy,
    pub expired_ask_side: ExpiredAsks,
    pub prior: String,
}

#[derive(Deserialize)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Deserialize)]
pub struct Verification {
    pub verified_sales: u64,
    pub minimum_sales: u64,
    pub verified_distinct_items: u64,
    pub minimum_distinct_items: u64,
}

#[derive(Deserialize)]
pub struct Sample {
    pub sales: u64,
    pub distinct_items: u64,
    pub median_scan_age_seconds: Option<i64>,
    pub median_gap_median_to_realized_bp: Option<i64>,
    pub median_gap_floor_to_realized_bp: Option<i64>,
    pub recent_move_groups: BTreeMap<String, SampleGroup>,
}

#[derive(Deserialize)]
pub struct SampleGroup {
    pub count: u64,
    pub median_gap_bp: Option<i64>,
}

#[derive(Deserialize)]
pub struct FloorProxy {
    pub thresholds: String,
    pub scope: String,
    pub state: String,
    pub at_or_near_floor_count: u64,
    pub decision_grade_at_floor_count: u64,
    pub minimum_at_floor_sales: u64,
    pub isolated_share: Ratio,
    pub isolated_minus_deep_realized_copper: Option<Copper>,
    pub groups: BTreeMap<String, FloorGroup>,
}

#[derive(Deserialize)]
pub struct FloorGroup {
    pub count: u64,
    pub median_realized_copper: Option<Copper>,
}

#[derive(Deserialize)]
pub struct ExpiredAsks {
    pub total_count: u64,
    pub excluded_by_reason: BTreeMap<String, u64>,
    pub rows: Vec<ExpiredAsk>,
}

#[derive(Deserialize)]
pub struct ExpiredAsk {
    pub listing_id: u64,
    pub pairing_scan_age_seconds: i64,
    pub gap_median_to_ask_bp: i64,
    pub gap_floor_to_ask_bp: i64,
}

#[derive(Deserialize)]
pub struct Accumulation {
    pub message: String,
    pub counts: BTreeMap<String, u64>,
}

fn read<T: serde::de::DeserializeOwned>(
    path: &str,
    params: &[(String, Document)],
    context: &Context<'_>,
    render: impl FnOnce(&T) -> Vec<String>,
) -> Result<CommandOutput> {
    let response: Payload<T> = crate::roster::roster(context.http, context.config, path, params)?;
    let mut lines = render(response.view());
    let out = response.into_document();
    let envelope: Envelope = out.decode("sales log attribution")?;
    lines.push(format!(
        "{} ({})",
        envelope.attribution.text, envelope.attribution.url
    ));
    let stale = is_stale(&sources(&out)?);
    let mut result = CommandOutput::new(out, lines);
    result.stale = stale;
    Ok(result)
}

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    use crate::human::saleslog as human;
    match &args.command {
        Command::Listings(window) => read(
            "/saleslog/listings",
            &window.query(),
            context,
            human::listings,
        ),
        Command::Outcomes(window) => read(
            "/saleslog/outcomes",
            &window.query(),
            context,
            human::outcomes,
        ),
        Command::Report(args) => read(
            "/saleslog/report",
            &[(
                "analysis".into(),
                args.analysis.map(Analysis::selector).into(),
            )],
            context,
            human::reports,
        ),
    }
}
