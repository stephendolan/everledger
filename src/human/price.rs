use crate::{
    commands::price::{MarketQuote, RoundedQuote},
    human::{copper_money, text, value},
    vendors::VendorAnswer,
};
pub fn render(id: i64, q: &MarketQuote, r: &RoundedQuote, vendors: &VendorAnswer) -> Vec<String> {
    let rate = q
        .sales
        .estimated_per_day
        .as_ref()
        .map(|n| format!("est {}/day", n.python_str()))
        .unwrap_or_else(|| "sales/day unknown".into());
    let coverage = q
        .sales
        .covered_hours
        .as_ref()
        .map(|n| format!("{}h coverage", n.python_str()))
        .unwrap_or_else(|| "coverage unknown".into());
    let mut lines = vec![format!(
        "{} (item {id}) latest {}: median {}, min {}, listed {} in {} auctions | 7d median {} | {rate} over {coverage}",
        text(q.item.name.as_deref()),
        text(q.latest.observed_at.as_deref()),
        copper_money(r.median),
        copper_money(r.min_buyout),
        value(q.latest.quantity.as_ref()),
        value(q.latest.auctions.as_ref()),
        copper_money(r.median_7d)
    )];
    match vendors {
        VendorAnswer::Ninja(a) => {
            let rows = a.coverage().rows();
            if !rows.is_empty() {
                lines.push(format!(
                    "vendor: listed price {} | 1.12 baseline sale/price; purchase quantity unknown",
                    copper_money(rows.iter().filter_map(|r| r.price_copper).min())
                ));
            }
        }
    }
    lines
}
