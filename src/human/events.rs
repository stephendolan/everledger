use crate::{
    catalogue::ItemRef,
    human::{boolean, copper_money, text},
    journal::{EventRow, Journal, JournalState, TimeBound},
};

pub const SNAPSHOT_RESTART: &str = "snapshot_changed: restarting the complete query once";
pub fn resync_notice(hint: &TimeBound) -> String {
    let hint = match hint {
        TimeBound::Epoch(n) => n.0.to_string(),
        TimeBound::Iso(s) => s.clone(),
    };
    format!("cursor_invalid: resynchronizing once from {hint}")
}
pub fn item_name(item: ItemRef<'_>) -> String {
    item.name
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| item.id.filter(|n| *n != 0).map(|n| n.to_string()))
        .unwrap_or_else(|| "unknown".into())
}
pub fn journal_unavailable(data: &JournalState) -> String {
    format!(
        "journal {}: {}",
        text(data.state.as_deref()),
        text(data.reason.as_deref())
    )
}
pub fn unavailable_line(data: &JournalState) -> Option<String> {
    data.unavailable().then(|| journal_unavailable(data))
}
pub fn event_lines(rows: &[EventRow]) -> Vec<String> {
    let lines = rows
        .iter()
        .map(|row| {
            let e = row.view();
            let item = e.item();
            let quantity = e
                .count
                .map_or_else(|| "quantity unknown".into(), |n| format!("{n}x"));
            format!(
                "{} | {} | {} | {quantity} {} | cash {}{}",
                text(e.actor()),
                text(e.recorded_at.as_deref()),
                e.event_type,
                item_name(item),
                copper_money(e.cash_copper),
                if e.matched_by_name() {
                    " | item matched by name"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec!["none recorded".into()]
    } else {
        lines
    }
}
pub fn lines(data: &Journal, rows: &[EventRow]) -> Vec<String> {
    let mut lines =
        unavailable_line(&data.view().state).map_or_else(|| event_lines(rows), |line| vec![line]);
    let d = &data.view().state;
    lines.push(format!(
        "next_cursor: {} | has_more: {} | after_cursor: {} | checkpoint: {}",
        text(d.next_cursor.as_deref()),
        boolean(d.has_more),
        text(d.after_cursor.as_deref()),
        text(d.checkpoint.as_deref())
    ));
    lines
}
