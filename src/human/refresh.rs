pub fn lines(sync: &crate::foreverdb::SyncData) -> Vec<String> {
    let mut lines = vec!["catalogue refreshed".into()];
    if sync.configured {
        lines.push(format!(
            "ForeverDB sync: {}; {} requests this refresh",
            if sync.complete { "complete" } else { "pending" },
            sync.requests
        ));
    }
    lines
}
pub fn sync_failed(status: Option<i64>) -> String {
    status.map_or_else(
        || "ForeverDB sync failed".into(),
        |s| format!("ForeverDB sync failed (HTTP {s})"),
    )
}
pub fn retaining(error: &crate::Failure) -> String {
    format!("{error}; retaining previous vendor caches")
}
pub fn credit([seen, baseline, unverified]: [usize; 3]) -> String {
    format!(
        "Data from ForeverDB.net (https://foreverdb.net); sell-row labels: {seen} seen in Forever, {baseline} Classic-only baseline, {unverified} unverified; vendor sightings separate; listed stack prices and quantities; faction from side"
    )
}
pub fn page_shape(profession: &str) -> String {
    format!("REFRESH FAILED for {profession} (page shape changed?)\n")
}

pub fn interrupted() -> &'static str {
    "ForeverDB sync interrupted"
}
