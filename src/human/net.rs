use crate::{
    commands::net::Totals,
    human::{copper_money, integer},
    journal::Journal,
};
pub fn lines(t: &Totals, journal: &Journal, by_name: bool) -> Vec<String> {
    let mut lines = vec![
        t.item
            .name
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| t.item.id.to_string()),
        t.posted.map_or_else(
            || "posted unknown".into(),
            |n| format!("posted {n} (posting attempts)"),
        ),
        format!(
            "sold {} | expired {} | cancelled {} | listed {}",
            integer(t.sold),
            integer(t.expired),
            integer(t.cancelled),
            integer(t.listed)
        ),
        format!(
            "unaccounted {} (signed diagnostic: posted - sold - expired - cancelled - listed)",
            integer(t.unaccounted)
        ),
        format!(
            "proceeds {} | auction cash {} before materials",
            copper_money(t.proceeds_copper),
            copper_money(t.cash_copper)
        ),
        "realized profit unknown; holding/materials basis unknown; real posting cycle unverified"
            .into(),
    ];
    if journal.view().available == Some(false) {
        lines.push(crate::human::events::journal_unavailable(journal));
    }
    if by_name {
        lines.push("item match basis: name (linkless journal rows)".into());
    }
    lines
}
