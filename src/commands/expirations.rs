#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "item", allow_negative_numbers = true)]
    pub item: Option<String>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "since", allow_negative_numbers = true)]
    pub since: Option<String>,
    #[arg(long = "until", allow_negative_numbers = true)]
    pub until: Option<String>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Result,
    catalogue::{disclose_item_source, item_for},
    commands::CommandOutput,
    document::Document,
    journal::{self, Selectors},
    provenance::sources,
};

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = args
        .item
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| item_for(s, context))
        .transpose()?;
    let params = journal::event_params(
        Selectors {
            character: args.char.as_deref(),
            since: args.since.as_deref(),
            until: args.until.as_deref(),
            venue: None,
            event_type: Some("expiration"),
            after: None,
            limit: None,
            default_since: Some("7d"),
        },
        context,
    )?;
    let data = journal::item_events(params, item.as_ref(), context)?;
    let mut rows = data.rows()?;
    let mut lines = crate::human::events::event_lines(&rows);
    let mut document = data.document().clone();
    document.remove("events");
    let mut stale = false;
    if !rows.is_empty() {
        let (prices, market_source) = crate::market::market_table(context)?;
        stale = market_source.view().stale == Some(true);
        let mut used = sources(&document)?;
        used.push(market_source);
        document.set("sources", journal::source_documents(used))?;
        for row in &mut rows {
            let price = row.view().item().id.and_then(|id| prices.get(&id));
            row.set("market", Document::from_typed(&price)?)?;
            if let Some(price) = price {
                lines.push(crate::human::expirations::repost(price));
            }
        }
    }
    document.set(
        "expirations",
        Document::Array(rows.into_iter().map(|r| r.into_document()).collect()),
    )?;
    lines.extend(crate::human::events::unavailable_line(
        &data.view().journal_state(),
    ));
    disclose_item_source(&mut document, item.as_ref(), context)?;
    let mut output = CommandOutput::new(document, lines);
    output.stale = stale;
    Ok(output)
}
