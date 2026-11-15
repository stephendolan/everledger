#[derive(clap::Args, Debug, Clone)]
#[command(
    args_override_self = true,
    after_help = "Start a watch: everledger events --limit 0 --json. Bootstrap drains a snapshot with positive service limits and prints no entries. Save after_cursor (the service terminal checkpoint) only after has_more is false. While has_more is true, pass next_cursor to --cursor to finish the snapshot; page cursors retain the local view. Character/item/type/venue filters are local views over an unfiltered checkpoint and may change between polls. Limit bounds the scanned page, so a filtered page may be empty with has_more true. Times are sent as integer epoch seconds. On 409 cursor_invalid, alert once and resync from resync_since; retries can replay entries. snapshot_changed requires restarting from the last committed checkpoint. Act before saving progress for at-least-once delivery. This CLI persists nothing."
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
use crate::{Context, Failure, Result, commands::CommandOutput};

pub fn run(args: &Args, _context: &Context<'_>) -> Result<CommandOutput> {
    if args.cursor.as_ref().is_some_and(|s| s.is_empty()) {
        return Err(Failure::usage(
            "empty --cursor; pass next_cursor from the previous page",
        ));
    }
    if args.limit.is_some_and(|n| n < 0) {
        return Err(Failure::usage("--limit must be nonnegative"));
    }
    Err(Failure::error("events: not yet ported"))
}
