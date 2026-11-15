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
    #[arg(
        long = "venue",
        allow_negative_numbers = true,
        value_enum,
        default_value = "auction"
    )]
    pub venue: Option<crate::types::Venue>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, Result,
    catalogue::{disclose_item_source, item_for},
    commands::CommandOutput,
    document::Document,
    journal::{self, Selectors},
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
            venue: journal::venue(args.venue),
            event_type: Some("sale"),
            after: None,
            limit: None,
            default_since: Some("7d"),
        },
        context,
    )?;
    let data = journal::item_events(params, item.as_ref(), context)?;
    let rows = journal::event_rows(&data)?;
    let mut lines = crate::human::events::event_lines(&rows);
    let mut document = data.document().clone();
    document.remove("events");
    let proceeds = if data.view().available == Some(false) {
        None
    } else {
        journal::money_total(rows.iter().map(|r| r.view().proceeds_copper))?
    };
    document.set("proceeds_copper", Document::from_typed(&proceeds)?)?;
    lines.push(crate::human::sales::total(proceeds));
    document.set(
        "sales",
        Document::Array(rows.into_iter().map(|r| r.into_document()).collect()),
    )?;
    if data.view().available == Some(false) {
        lines.push(crate::human::events::journal_unavailable(&data));
    }
    disclose_item_source(&mut document, item.as_ref(), context)?;
    Ok(CommandOutput::new(document, lines))
}
