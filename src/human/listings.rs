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
            let item = r.item.item.as_ref();
            let name = item
                .and_then(|i| i.name.as_ref())
                .filter(|s| !s.is_empty())
                .or(r.item.name.as_ref().filter(|s| !s.is_empty()))
                .cloned()
                .or_else(|| {
                    item.and_then(|i| i.id.as_ref())
                        .copied()
                        .filter(|id| *id != 0)
                        .or(r.item.item_id)
                        .map(|id| id.to_string())
                });
            let quantity = r
                .count
                .map_or_else(|| "quantity unknown".into(), |n| format!("{n}x"));
            format!(
                "{} | {quantity} {} | buyout {} | expires {} | floor {} | undercut {}",
                text(r.character.as_deref()),
                text(name.as_deref()),
                copper_money(r.buyout_copper),
                text(r.expires_at.as_deref()),
                copper_money(r.floor_copper),
                boolean(r.undercut)
            )
        })
        .collect()
}
