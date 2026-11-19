use crate::{
    commands::listings::ListingRow,
    human::{boolean, copper_money, text},
};
pub fn lines(rows: &[ListingRow]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["none recorded".into()];
    }
    rows.iter()
        .map(|r| {
            let r = r.view();
            let item = r.row.item();
            let name = item
                .name
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| item.id.map(|id| id.to_string()));
            let quantity = r
                .row
                .count
                .map_or_else(|| "quantity unknown".into(), |n| format!("{n}x"));
            format!(
                "{} | {quantity} {} | buyout {} | expires {} | floor {} | undercut {}",
                text(r.row.character.as_deref()),
                text(name.as_deref()),
                copper_money(r.buyout_copper),
                text(r.expires_at.as_deref()),
                copper_money(r.floor_copper),
                boolean(r.undercut)
            )
        })
        .collect()
}
