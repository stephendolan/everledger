use crate::{
    human::{copper_money, text, value},
    vendors::{Basis, Coverage, VendorAnswer},
};
pub fn render(answer: &VendorAnswer) -> Vec<String> {
    match answer {
        VendorAnswer::Ninja(a) => {
            let rows = match a.coverage() {
                Coverage::ItemAbsent | Coverage::PinsAbsent => &[][..],
                Coverage::Sellers(rows) => rows.as_slice(),
            };
            if rows.is_empty() {
                let mut lines = vec![
                    match a.coverage() {
                        Coverage::PinsAbsent => "vendor coverage unknown on wowforever.ninja",
                        Coverage::ItemAbsent | Coverage::Sellers(_) => {
                            "no known sellers in the vendor index"
                        }
                    }
                    .into(),
                ];
                if let Some(zones) = a
                    .payload()
                    .view()
                    .known_zones
                    .as_ref()
                    .filter(|z| !z.is_empty())
                {
                    lines.push(format!("known zones: {}", zones.join(", ")));
                }
                return lines;
            }
            let item = &a.payload().view().item;
            let name = item
                .name
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| item.id.to_string());
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
