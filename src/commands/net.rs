#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers=true,required=true,num_args=1..)]
    pub item: Vec<String>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "since", allow_negative_numbers = true)]
    pub since: Option<String>,
    #[arg(long = "until", allow_negative_numbers = true)]
    pub until: Option<String>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Copper, Failure, Result,
    catalogue::{self, Item, item_matches},
    commands::{
        CommandOutput,
        listings::{self, listing_rows, listing_sources},
    },
    document::Document,
    journal::{self, Selectors},
    provenance::{Field, is_stale, sources},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Totals {
    pub item: Item,
    pub posted: Option<i64>,
    pub sold: Option<i64>,
    pub expired: Option<i64>,
    pub cancelled: Option<i64>,
    pub listed: Option<i64>,
    pub unaccounted: Option<i64>,
    pub cash_copper: Option<Copper>,
    pub proceeds_copper: Option<Copper>,
    pub deposits_lost_copper: Option<Copper>,
}
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = catalogue::load_catalogue(context)?;
    let item = catalogue::item_for(&args.item.join(" "), context)?;
    let params = journal::event_params(
        Selectors {
            character: args.char.as_deref(),
            since: args.since.as_deref(),
            until: args.until.as_deref(),
            venue: None,
            event_type: None,
            after: None,
            limit: None,
            default_since: Some("7d"),
        },
        context,
    )?;
    let data = journal::item_events(params, Some(&item), context)?;
    let rows = data.rows()?;
    let current = listings::listings(
        args.char.as_deref(),
        Some(&item.id.to_string()),
        None,
        context,
    )?;
    let listed_rows = listing_rows(&current)?
        .into_iter()
        .filter(|r| item_matches(r.view().item(), &item))
        .collect::<Vec<_>>();
    let by_type = ["posting", "sale", "expiration", "cancellation"].map(|typ| {
        rows.iter()
            .filter(|e| e.view().event_type == typ && e.view().venue.as_deref() == Some("auction"))
            .map(|e| e.view())
            .collect::<Vec<_>>()
    });
    let available = !data.view().journal_state().unavailable();
    let mut counts = [None; 4];
    if available {
        for (i, events) in by_type.iter().enumerate() {
            counts[i] = if i == 0 && events.is_empty() {
                None
            } else {
                journal::count_total(events.iter().copied())?
            };
        }
    }
    let listing_known = match current.view().available {
        Field::Missing => true,
        Field::Null => false,
        Field::Value(v) => v,
    } && !listing_sources(&current)?
        .iter()
        .any(|s| s.view().stale == Some(true))
        && current.view().scans.iter().all(|s| {
            s.available == Some(true)
                && s.pricing
                    .as_ref()
                    .is_some_and(|p| p.available.as_ref() == Some(&true))
        })
        && listed_rows
            .iter()
            .all(|r| r.view().available.as_ref() != Some(&false));
    let listed = if listing_known {
        listed_rows
            .iter()
            .try_fold(Some(0), |n, r| journal::checked_count(n, r.view().count))?
    } else {
        None
    };
    let unaccounted = match (counts, listed) {
        ([Some(posted), Some(sold), Some(expired), Some(cancelled)], Some(listed)) => Some(
            [sold, expired, cancelled, listed]
                .into_iter()
                .try_fold(posted, |n, v| {
                    n.checked_sub(v)
                        .ok_or_else(|| Failure::error("count out of range"))
                })?,
        ),
        _ => None,
    };
    let totals = Totals {
        item,
        posted: counts[0],
        sold: counts[1],
        expired: counts[2],
        cancelled: counts[3],
        listed,
        unaccounted,
        cash_copper: if available {
            journal::money_total(by_type.iter().flatten().map(|e| e.cash_copper))?
        } else {
            None
        },
        proceeds_copper: if available {
            journal::money_total(by_type[1].iter().map(|e| e.proceeds_copper))?
        } else {
            None
        },
        deposits_lost_copper: None,
    };
    let mut match_basis = rows
        .iter()
        .map(|r| journal::basis(r.view().item()))
        .collect::<Vec<_>>();
    match_basis.sort_unstable();
    match_basis.dedup();
    let lines = crate::human::net::lines(&totals, &data, match_basis.contains(&"name"));
    let materials = catalogue::recipes(cat)?
        .into_iter()
        .filter(|r| r.recipe.result.item_id == Some(totals.item.id))
        .map(|r| {
            r.document
                .get("reagents")
                .cloned()
                .ok_or_else(|| Failure::shape("recipe", "missing reagents"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut document = data.document().clone();
    for key in ["events", "matched", "returned", "counts"] {
        document.remove(key);
    }
    let totals_doc = Document::from_typed(&totals)?;
    if let Some(entries) = totals_doc.as_object() {
        for (key, value) in entries {
            document.set(key, value.clone())?;
        }
    }
    document.set("materials", Document::Array(materials))?;
    document.set("materials_copper", Document::Null)?;
    document.set("realized_profit_copper", Document::Null)?;
    document.set("basis", "unknown: no supported lot assignments")?;
    document.set(
        "reconciliation",
        "unverified: real posting/invoice/expiry cycle required",
    )?;
    document.set("match_basis", Document::from_typed(&match_basis)?)?;
    let stale = is_stale(&listing_sources(&current)?);
    document.set("stale", stale)?;
    let mut used = sources(data.document())?;
    used.extend(sources(current.document())?);
    used.push(catalogue::catalogue_source(cat, context.clock)?);
    document.set("sources", journal::source_documents(used))?;
    let mut output = CommandOutput::new(document, lines);
    output.stale = stale;
    Ok(output)
}
