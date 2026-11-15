use crate::{
    catalogue::RecordedItem,
    human::{boolean, copper_money, text},
    journal::{EventRow, Journal, TimeBound},
};

pub const SNAPSHOT_RESTART: &str = "snapshot_changed: restarting the complete query once";
pub fn resync_notice(hint: &TimeBound) -> String {
    let hint = match hint {
        TimeBound::Epoch(n) => n.0.to_string(),
        TimeBound::Iso(s) => s.clone(),
    };
    format!("cursor_invalid: resynchronizing once from {hint}")
}
pub fn item_name(item: Option<&RecordedItem>) -> String {
    item.and_then(|i| i.name.as_ref())
        .filter(|s| !s.is_empty())
        .cloned()
        .or_else(|| {
            item.and_then(|i| i.id.as_ref())
                .filter(|n| **n != 0)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}
pub fn journal_unavailable(data: &Journal) -> String {
    format!(
        "journal {}: {}",
        text(data.view().state.as_deref()),
        text(data.view().reason.as_deref())
    )
}
pub fn event_lines(rows: &[EventRow]) -> Vec<String> {
    let lines = rows
        .iter()
        .map(|row| {
            let e = row.view();
            let item = e.item.item.as_ref();
            let by_name = e.match_basis.as_deref() == Some("name")
                || item.is_some_and(|i| {
                    i.id_basis.as_deref() == Some("name")
                        || i.id.as_ref().is_none() && i.name.as_ref().is_some_and(|s| !s.is_empty())
                });
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
                if by_name {
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
    let mut lines = if data.view().available == Some(false) {
        vec![journal_unavailable(data)]
    } else {
        event_lines(rows)
    };
    let d = data.view();
    lines.push(format!(
        "next_cursor: {} | has_more: {} | after_cursor: {} | checkpoint: {}",
        text(d.next_cursor.as_deref()),
        boolean(d.has_more),
        text(d.after_cursor.as_deref()),
        text(d.checkpoint.as_deref())
    ));
    lines
}
