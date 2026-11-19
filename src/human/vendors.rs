use crate::{
    human::{copper_money, text, value},
    vendors::{Basis, Coverage, VendorAnswer},
};
pub fn render(answer: &VendorAnswer) -> Vec<String> {
    match answer {
        VendorAnswer::Foreverdb(a) => foreverdb(a),
        VendorAnswer::Ninja(a) => {
            let rows = a.rows();
            if rows.is_empty() {
                let mut lines = vec![
                    match a.coverage() {
                        Coverage::PinsAbsent => "vendor coverage unknown on wowforever.ninja",
                        Coverage::ItemAbsent | Coverage::Sellers => {
                            "no known sellers in the vendor index"
                        }
                    }
                    .into(),
                ];
                append_known_zones(&mut lines, a.payload().view().known_zones.as_deref());
                return lines;
            }
            let item = &a.payload().view().item;
            let name = item_name(item);
            let mut lines = vec![format!(
                "{name} — vendor listed prices (sale and price: 1.12 baseline; faction unavailable)"
            )];
            for v in rows {
                let coords = v
                    .coords
                    .first()
                    .map(|c| {
                        c.iter()
                            .map(|n| value(n.as_ref()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "coordinates unknown".into());
                let extra = if v.coords.len() > 1 {
                    format!(" (+{} locations)", v.coords.len() - 1)
                } else {
                    String::new()
                };
                let basis = match v.basis {
                    Basis::Seen => "vendor seen in Forever",
                    Basis::Baseline => "1.12 baseline",
                    Basis::Unverified => "unverified",
                };
                lines.push(format!(
                    "  {}: {} ({}) {coords}{extra} | listed price {} | {basis}",
                    text(Some(&v.zone)),
                    text(Some(&v.name)),
                    text(Some(&v.title)),
                    copper_money(v.price_copper)
                ));
            }
            lines
        }
    }
}

pub fn fallback_detail(detail: &str) -> String {
    format!("{detail}; ForeverDB sync failed; ninja fallback")
}
pub fn listed_price(row: &crate::foreverdb::Seller) -> String {
    if row.price_copper.is_some() {
        copper_money(row.price_copper)
    } else if row.stack_price_copper.is_some() {
        format!("{} per {}", copper_money(row.stack_price_copper), row.stack)
    } else {
        "unknown".into()
    }
}
pub fn stock_label(row: &crate::foreverdb::Seller) -> String {
    if !row.limited {
        String::new()
    } else {
        format!(
            " | limited stock{}",
            row.limited_stock_count
                .map_or_else(String::new, |n| format!(" ({n})"))
        )
    }
}
fn foreverdb(answer: &crate::vendors::ForeverdbAnswer) -> Vec<String> {
    let rows = answer.rows();
    let mut lines = vec![];
    if rows.is_empty() {
        lines.push("no known sellers in the vendor index".into());
        append_known_zones(&mut lines, answer.payload().view().known_zones.as_deref());
    } else {
        let item = &answer.payload().view().item;
        let name = item_name(item);
        lines.push(format!(
            "{name} — vendor listed prices (sale/price from ForeverDB sell-row labels)"
        ));
        for row in rows {
            let coords = row
                .coords
                .first()
                .map(|c| {
                    c.iter()
                        .map(|n| value(Some(n)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "coordinates unknown".into());
            let locations = format!("{}: {coords}", text(Some(&row.zone)));
            lines.push(format!(
                "  {} ({}) | {locations} | {} | listed price {}{} | sale/price {}; vendor {}",
                text(Some(&row.name)),
                value(row.title.as_ref()),
                value(row.side.as_ref()),
                listed_price(row),
                stock_label(row),
                row.sale_basis,
                row.vendor_basis.name()
            ));
        }
    }
    let additional = answer.payload().view().fallback_additional_sellers;
    if additional > 0 {
        lines.push(format!(
            "Fallback dataset knows {additional} more seller{} for this item.",
            if additional == 1 { "" } else { "s" }
        ));
    }
    lines
}

fn item_name(item: &crate::foreverdb::Item) -> String {
    item.name
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| item.id.to_string())
}
fn append_known_zones(lines: &mut Vec<String>, zones: Option<&[String]>) {
    if let Some(zones) = zones.filter(|z| !z.is_empty()) {
        lines.push(format!("known zones: {}", zones.join(", ")));
    }
}
