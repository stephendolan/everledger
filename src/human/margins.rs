use crate::{
    commands::margins::Margin,
    human::{bridge::color, copper_money, text},
};
pub fn render(rows: &[Margin<'_>]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["no priced recipes".into()];
    }
    rows.iter()
        .map(|r| {
            format!(
                "{} [{}] {} | margin {} | cost {} | sells {} | {} | {}",
                r.recipe.name,
                r.profession,
                text(r.color.map(color)),
                copper_money(Some(r.margin)),
                copper_money(Some(r.cost)),
                copper_money(Some(r.sells)),
                r.sales
                    .map(|s| format!("{}/day", crate::primitives::float_repr(s)))
                    .unwrap_or_else(|| "sales/day unknown".into()),
                text(r.recipe.source_type.as_deref())
            )
        })
        .collect()
}
