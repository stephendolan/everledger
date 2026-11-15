use crate::{commands::positions::Position, human::copper_money};
pub fn lines(rows: &[Position], complete: bool) -> Vec<String> {
    let mut lines = rows
        .iter()
        .map(|r| {
            let name = r
                .item
                .view()
                .name
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    r.item
                        .view()
                        .id
                        .map_or_else(|| "None".into(), |id| id.to_string())
                });
            format!(
                "{name} | recorded held {} | listed {} | basis unknown | floor {} | held value {}",
                r.held,
                r.listed,
                copper_money(r.floor_copper),
                copper_money(r.value_copper)
            )
        })
        .collect::<Vec<_>>();
    if !complete {
        lines.push("holdings incomplete: some inventory locations are unavailable".into());
    }
    if lines.is_empty() {
        lines.push("none recorded".into());
    }
    lines
}
