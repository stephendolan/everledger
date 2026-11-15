#[derive(clap::Args, Debug, Clone)]
#[command(
    args_override_self = true,
    after_help = "Start a watch: everledger events --limit 0 --json. Bootstrap drains a snapshot\nwith positive service limits and prints no entries. Save after_cursor (the\nservice terminal checkpoint) only after has_more is false. While has_more is\ntrue, pass next_cursor to --cursor to finish the snapshot; page cursors retain\nthe local view. Character/item/type/venue filters are local views over an\nunfiltered checkpoint and may change between polls. Limit bounds the scanned\npage, so a filtered page may be empty with has_more true. Times are sent as\ninteger epoch seconds. On 409 cursor_invalid, alert once and resync from\nresync_since; retries can replay entries. snapshot_changed requires restarting\nfrom the last committed checkpoint. Act before saving progress for\nat-least-once delivery. This CLI persists nothing."
)]
pub struct Args {
    #[arg(long = "item", allow_negative_numbers = true)]
    pub item: Option<String>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "since", allow_negative_numbers = true)]
    pub since: Option<String>,
    #[arg(long = "until", allow_negative_numbers = true)]
    pub until: Option<String>,
    #[arg(long = "venue", allow_negative_numbers = true, value_enum)]
    pub venue: Option<crate::types::Venue>,
    #[arg(
        long = "type",
        allow_negative_numbers = true,
        help = "comma-separated purchase, sale, posting, expiration, cancellation"
    )]
    pub event_type: Option<String>,
    #[arg(
        long = "cursor",
        allow_negative_numbers = true,
        help = "snapshot page cursor from next_cursor; inherits fixed filters"
    )]
    pub cursor: Option<String>,
    #[arg(
        long = "after",
        allow_negative_numbers = true,
        help = "terminal after_cursor/checkpoint; exclusive with since/until"
    )]
    pub after: Option<String>,
    #[arg(long="limit",value_parser=crate::cli::parse_integer,allow_negative_numbers=true,help="0 bootstraps a watch; default 50 human, 500 JSON")]
    pub limit: Option<i64>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, Failure, Result,
    catalogue::{disclose_item_source, item_for},
    commands::CommandOutput,
    journal::{self, ItemView, PageOrder, Selectors},
};

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    if args.cursor.as_ref().is_some_and(|s| s.is_empty()) {
        return Err(Failure::usage(
            "empty --cursor; pass next_cursor from the previous page",
        ));
    }
    if args.limit.is_some_and(|n| n < 0) {
        return Err(Failure::usage("--limit must be nonnegative"));
    }
    let limit = args
        .limit
        .or(if args.cursor.as_deref().is_some_and(|s| !s.is_empty()) {
            None
        } else {
            Some(if args.json { 500 } else { 50 })
        });
    let item = args
        .item
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| item_for(s, context))
        .transpose()?;
    let mut params = journal::event_params(
        Selectors {
            character: args.char.as_deref(),
            since: args.since.as_deref(),
            until: args.until.as_deref(),
            venue: journal::venue(args.venue),
            event_type: args.event_type.as_deref(),
            after: args.after.as_deref(),
            limit,
            default_since: if limit == Some(0) { None } else { Some("24h") },
        },
        context,
    )?;
    params.view.item = item.as_ref().map(ItemView::resolved).transpose()?;
    if let Some(cursor) = args.cursor.as_deref().filter(|s| !s.is_empty()) {
        if args.after.is_some()
            || [
                &args.since,
                &args.until,
                &args.char,
                &args.item,
                &args.event_type,
            ]
            .iter()
            .any(|s| s.as_ref().is_some_and(|s| !s.is_empty()))
            || args.venue.is_some()
        {
            return Err(Failure::usage(
                "--cursor inherits filters; supply only --cursor and --limit",
            ));
        }
        params = journal::page_params(cursor, limit, PageOrder::CursorLimit)?;
    }
    let data = journal::fetch_events(params, context)?;
    let lines = crate::human::events::lines(&data, &journal::event_rows(&data)?);
    let mut document = data.into_document();
    disclose_item_source(&mut document, item.as_ref(), context)?;
    Ok(CommandOutput::new(document, lines))
}
