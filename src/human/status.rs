use crate::{
    commands::status::StatusRow,
    human::{boolean, text},
    vendors::status::{BackendStatusData, ForeverdbState},
};
pub fn ninja_detail(items: usize, state: ForeverdbState) -> String {
    format!("cache {items} items; ForeverDB {}", state.name())
}
pub fn render(rows: &[StatusRow], vendor: &BackendStatusData) -> Vec<String> {
    let mut lines = rows
        .iter()
        .map(|r| {
            if let Some(line) = &r.line {
                return match &r.market_source {
                    Some(state) => format!("{line} | AHledger {state}"),
                    None => line.clone(),
                };
            }
            let mut parts = vec![text(Some(&r.source))];
            if let Some(backend) = r.backend {
                parts.push(backend.name().into());
            }
            parts.push(text(r.state.as_ref().map(String::as_str)));
            parts.join(" | ")
        })
        .collect::<Vec<_>>();
    let info = &vendor.foreverdb;
    lines.push(format!("ForeverDB {} | transport {} | last sync {} | {} vendor records | {} item records | pending {}",info.state.name(),info.transport.map_or("none",|t|t.name()),info.last_sync_at.as_deref().filter(|s|!s.is_empty()).unwrap_or("never"),info.record_count,info.item_count,boolean(Some(info.pending))));
    let ninja = &vendor.coverage.ninja;
    lines.push(format!(
        "wowforever.ninja cache | {} vendor records | {} item records",
        ninja.record_count, ninja.item_count
    ));
    if vendor.cache_items == 0 && vendor.published_at.as_deref().is_none_or(str::is_empty) {
        lines.push("vendors data: no cache yet".into());
    } else {
        lines.push(format!(
            "vendors data {} | cache {} items | {} seen, {} baseline, {} unverified",
            text(vendor.published_at.as_deref()),
            vendor.cache_items,
            vendor.seen,
            vendor.baseline,
            vendor.unverified
        ));
    }
    lines
}
