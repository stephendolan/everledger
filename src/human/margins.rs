use crate::{
    commands::margins::Margin,
    human::{bridge::color, copper_money, text},
};
pub fn render(rows: &[Margin<'_>]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["no matching recipes".into()];
    }
    rows.iter()
        .map(|r| {
            let mut margin = copper_money(r.planning.margin);
            if let Some(reason) = r.planning.reason {
                margin.push_str(&format!(" ({})", reason.replace('_', " ")));
            }
            let sell = match r.planning.sell {
                Some((price, basis)) => {
                    let label = match basis {
                        "current_floor" => "current floor",
                        "current_median" => "current median",
                        "7d_median" => "7-day median",
                        _ => basis,
                    };
                    format!("{} ({label})", copper_money(Some(price)))
                }
                None => copper_money(None),
            };
            format!(
                "{} [{}] {} | planning margin {margin} | current reagent cost {} | planning sell {sell} | {} | {}",
                r.recipe.name,
                r.profession,
                text(r.color.map(color)),
                copper_money(r.planning.cost),
                r.planning.sales
                    .map(|s| format!("{}/day", crate::primitives::float_repr(s)))
                    .unwrap_or_else(|| "sales/day unknown".into()),
                text(r.recipe.source_type.as_deref())
            )
        })
        .collect()
}
