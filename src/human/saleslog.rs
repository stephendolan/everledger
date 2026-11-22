use crate::{
    commands::saleslog::{AnalysisA, Listings, Outcomes, Report, Reports, Sample},
    human::{copper_money, text},
};

fn percent(bp: Option<i64>) -> String {
    bp.map_or_else(
        || "unknown".into(),
        |n| {
            let absolute = n.unsigned_abs();
            let sign = if n < 0 { "-" } else { "" };
            format!("{sign}{}.{:02}%", absolute / 100, absolute % 100)
        },
    )
}

fn age(seconds: Option<i64>) -> String {
    seconds.map_or_else(|| "age unknown".into(), |n| format!("age {n}s"))
}

fn interval(start: Option<&str>, end: Option<&str>) -> String {
    format!("[{}, {}]", text(start), text(end))
}

fn pairing(status: &str, scan: Option<&str>, seconds: Option<i64>) -> String {
    match scan {
        Some(scan) => format!("{status} at scan {scan} ({})", age(seconds)),
        None => status.to_owned(),
    }
}

pub fn listings(view: &Listings) -> Vec<String> {
    let mut lines = vec![format!("{} first-party listings", view.listings.len())];
    for row in &view.listings {
        lines.push(format!(
            "#{} {} | {} × {} at {}/unit | posting {} ({})",
            row.listing_id,
            row.character_name,
            row.quantity_total,
            row.item_name_snapshot,
            copper_money(Some(row.posted_price_per_unit_copper)),
            interval(
                row.posted_at_earliest.as_deref(),
                row.posted_at_latest.as_deref()
            ),
            row.posted_at_precision,
        ));
        lines.push(format!(
            "  {} | floor {} | median {} | position {}",
            pairing(
                &row.pairing_status,
                row.pairing_scan_ts.as_deref(),
                row.pairing_scan_age_seconds
            ),
            copper_money(row.floor_at_pairing_copper),
            copper_money(row.median_at_pairing_copper),
            percent(row.price_position_bp),
        ));
    }
    lines
}

pub fn outcomes(view: &Outcomes) -> Vec<String> {
    let mut lines = vec![format!("{} first-party outcomes", view.outcomes.len())];
    for row in &view.outcomes {
        let listing = row
            .listing_id
            .map_or_else(String::new, |n| format!(" listing #{n}"));
        lines.push(format!(
            "{} | {} | {} × {} | {}{} | mail collected {}",
            row.outcome_type,
            row.character_name,
            row.units_involved,
            row.item_name_snapshot,
            row.linkage_status,
            listing,
            text(row.collected_at.as_deref()),
        ));
        if row.outcome_type == "sold" {
            lines.push(format!(
                "  sale interval {} ({}) | gross {} | net {} | basis {}",
                interval(
                    row.sale_interval_start.as_deref(),
                    row.sale_interval_end.as_deref()
                ),
                text(row.sale_interval_source.as_deref()),
                copper_money(row.gross_proceeds_copper),
                copper_money(row.net_proceeds_copper),
                text(row.proceeds_basis.as_deref()),
            ));
        } else if row.outcome_type == "purchased" {
            lines.push(format!(
                "  purchase price {}",
                copper_money(row.purchase_price_copper)
            ));
        }
        lines.push(format!(
            "  collection context {}",
            pairing(
                &row.pairing_status,
                row.pairing_scan_ts.as_deref(),
                row.pairing_scan_age_seconds
            ),
        ));
    }
    lines
}

fn sample(label: &str, sample: &Sample) -> String {
    let groups = sample
        .recent_move_groups
        .iter()
        .map(|(name, group)| {
            format!(
                "{name} {} (gap {})",
                group.count,
                percent(group.median_gap_bp)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{label} | {} sales across {} items | median scan {} | median-to-realized {} | floor-to-realized {} | {groups}",
        sample.sales,
        sample.distinct_items,
        age(sample.median_scan_age_seconds),
        percent(sample.median_gap_median_to_realized_bp),
        percent(sample.median_gap_floor_to_realized_bp),
    )
}

fn analysis_a(report: &AnalysisA) -> Vec<String> {
    let gate = &report.linkage_gate;
    let checked = &report.verification;
    let proxy = &report.floor_proxy;
    let expired = &report.expired_ask_side;
    let mut lines = vec![
        format!(
            "Analysis A | {} | verdict {} ({})",
            report.state,
            report.verdict,
            report.reason.replace('_', " ")
        ),
        report.scope.clone(),
        format!(
            "{} total | {} decision-grade | {} excluded | proceeds basis {}",
            report.total_count,
            report.decision_grade_count,
            report.excluded_count,
            report.proceeds_basis
        ),
        format!(
            "Linked {}/{} | November verification {}/{} sales across {}/{} items",
            gate.numerator,
            gate.denominator,
            checked.verified_sales,
            checked.minimum_sales,
            checked.verified_distinct_items,
            checked.minimum_distinct_items
        ),
    ];
    lines.extend(
        report
            .excluded_by_reason
            .iter()
            .map(|(reason, count)| format!("  excluded {}: {count}", reason.replace('_', " "))),
    );
    lines.push(sample("Listing-time scans", &report.listing_time));
    lines.push(sample(
        "Scan age at most one hour",
        &report.at_most_one_hour,
    ));
    if report.scan_age_sensitive {
        lines.push("The fallen-versus-risen result is scan-age-sensitive.".into());
    }
    lines.push(format!(
        "Floor proxy ({}) | {} at or near the floor | {}/{} decision-grade at-floor sales | isolated share {}/{}",
        proxy.state, proxy.at_or_near_floor_count, proxy.decision_grade_at_floor_count, proxy.minimum_at_floor_sales,
        proxy.isolated_share.numerator, proxy.isolated_share.denominator,
    ));
    lines.push(proxy.thresholds.clone());
    lines.push(proxy.scope.clone());
    lines.extend(proxy.groups.iter().map(|(name, group)| {
        format!(
            "  {}: {} sales, median realized {}",
            name.replace('_', " "),
            group.count,
            copper_money(group.median_realized_copper)
        )
    }));
    if let Some(value) = proxy.isolated_minus_deep_realized_copper {
        lines.push(format!(
            "  isolated minus deep median realized {}",
            copper_money(Some(value))
        ));
    }
    let excluded: u64 = expired.excluded_by_reason.values().sum();
    lines.push(format!(
        "Expired asks | {} total | {excluded} excluded",
        expired.total_count
    ));
    lines.extend(
        expired
            .excluded_by_reason
            .iter()
            .map(|(reason, count)| format!("  excluded {}: {count}", reason.replace('_', " "))),
    );
    lines.extend(expired.rows.iter().map(|row| {
        format!(
            "  listing #{} | scan {} | median-to-ask {} | floor-to-ask {}",
            row.listing_id,
            age(Some(row.pairing_scan_age_seconds)),
            percent(Some(row.gap_median_to_ask_bp)),
            percent(Some(row.gap_floor_to_ask_bp))
        )
    }));
    lines.push(report.prior.clone());
    lines
}

pub fn reports(view: &Reports) -> Vec<String> {
    let mut lines = Vec::new();
    for report in &view.reports {
        match report {
            Report::A(report) => lines.extend(analysis_a(report)),
            Report::B(pending) | Report::C(pending) => {
                let name = if matches!(report, Report::B(_)) {
                    "B"
                } else {
                    "C"
                };
                let counts = pending
                    .counts
                    .iter()
                    .map(|(table, n)| format!("{n} {}", table.replace('_', " ")))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("Analysis {name} | {} {counts}", pending.message));
            }
        }
    }
    lines
}
