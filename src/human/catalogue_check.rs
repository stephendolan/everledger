use crate::{catalogue::Catalogue, catalogue_check::Check, human::text};
pub fn render(checks: &[Check<'_>], cat: &Catalogue) -> Vec<String> {
    let mut lines = checks
        .iter()
        .map(|check| {
            let status = if check.good() { "PASS" } else { "FAIL" };
            match check {
                Check::Count {
                    profession,
                    actual,
                    stated,
                    ..
                } => format!("{status} {profession}: {actual}/{stated} recipes"),
                Check::Anchor { name, recipe, .. } => {
                    let (learn, source) = match recipe {
                        Some(r) => (r.bands.learn.to_string(), text(r.source_type.as_deref())),
                        None => ("?".into(), "?".into()),
                    };
                    format!("{status} anchor {name}: learn {learn}, source {source}")
                }
            }
        })
        .collect::<Vec<_>>();
    lines.push(format!(
        "catalogue built: {} | source: {}",
        cat.view().generated_at,
        cat.view()
            .source
            .as_ref()
            .map(String::as_str)
            .unwrap_or("None")
    ));
    lines
}
