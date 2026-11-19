#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "item", allow_negative_numbers = true)]
    pub item: Option<String>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Copper, Result,
    catalogue::{ItemRow, disclose_item_source, item_for},
    commands::CommandOutput,
    document::Document,
    http::Payload,
    journal::{recount, row_payloads, source_documents},
    market::PriceTable,
    provenance::{Field, Source, SourceData, is_stale, sources},
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct RecordedPrice {
    pub buyout_copper: Option<Copper>,
    pub bid_copper: Option<Copper>,
    pub has_bid: Option<bool>,
    pub deposit_copper: Option<Copper>,
    pub expires_at: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    pub available: Field<bool>,
    pub source: Option<SourceData>,
    pub recorded: Option<RecordedPrice>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Scan {
    pub available: Option<bool>,
    pub pricing: Option<Pricing>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Listing {
    #[serde(flatten)]
    pub item: ItemRow,
    pub character: Option<String>,
    pub count: Option<i64>,
    pub buyout_copper: Option<Copper>,
    pub bid_copper: Option<Copper>,
    pub has_bid: Option<bool>,
    pub deposit_copper: Option<Copper>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub available: Field<bool>,
    pub floor_copper: Option<Copper>,
    pub undercut: Option<bool>,
    pub price: Option<Pricing>,
}
pub type ListingRow = Payload<Listing>;
#[derive(Clone, Debug, Deserialize)]
pub struct ListingData {
    #[serde(default)]
    pub listings: Vec<Listing>,
    #[serde(default)]
    pub scans: Vec<Scan>,
    #[serde(default)]
    pub available: Field<bool>,
}
pub type Listings = Payload<ListingData>;

fn pricing_sources(data: &Listings, key: &str, pricing_key: &str) -> Result<Vec<Source>> {
    let documents = data
        .document()
        .get(key)
        .and_then(Document::as_array)
        .unwrap_or(&[]);
    documents
        .iter()
        .filter_map(|r| {
            r.get(pricing_key)
                .and_then(|p| p.get("source"))
                .filter(|s| s.truthy())
        })
        .map(|s| Payload::decode(s.clone(), "listing pricing source"))
        .collect()
}
pub fn listing_sources(data: &Listings) -> Result<Vec<Source>> {
    let scans = pricing_sources(data, "scans", "pricing")?;
    if !scans.is_empty() {
        return Ok(scans);
    }
    let rows = pricing_sources(data, "listings", "price")?;
    if !rows.is_empty() {
        return Ok(rows);
    }
    sources(data.document())
}
pub fn listing_rows(data: &Listings) -> Result<Vec<ListingRow>> {
    row_payloads(data.document(), "listings", "listing")
}
pub fn listings(
    character: Option<&str>,
    item: Option<&str>,
    prices: Option<&PriceTable>,
    context: &Context<'_>,
) -> Result<Listings> {
    let mut data: Listings = crate::roster::roster(
        context.http,
        context.config,
        "/listings",
        &[
            ("char".into(), Document::from(character)),
            ("item".into(), Document::from(item)),
        ],
    )?;
    let mut used = sources(data.document())?;
    used.extend(pricing_sources(&data, "scans", "pricing")?);
    let mut unique: Vec<Source> = vec![];
    for source in used {
        if !unique.iter().any(|s| s.document() == source.document()) {
            unique.push(source);
        }
    }
    let used = unique;
    data.set("sources", source_documents(used))?;
    let mut rows = listing_rows(&data)?;
    for row in &mut rows {
        let id = match row.view().item.item.as_ref().map(|i| &i.id) {
            None | Some(Field::Missing) => row.view().item.item_id,
            Some(Field::Null) => None,
            Some(Field::Value(id)) => Some(*id),
        };
        let floor = id
            .and_then(|id| prices.and_then(|p| p.get(&id)))
            .and_then(|p| p.floor_copper);
        let recorded = row
            .document()
            .get("price")
            .and_then(|p| p.get("recorded"))
            .cloned();
        for key in [
            "buyout_copper",
            "bid_copper",
            "has_bid",
            "deposit_copper",
            "expires_at",
        ] {
            row.set_default(
                key,
                recorded
                    .as_ref()
                    .and_then(|r| r.get(key))
                    .cloned()
                    .unwrap_or(Document::Null),
            )?;
        }
        let available = row.view().price.as_ref().map(|p| &p.available);
        let available = match available {
            None | Some(Field::Missing) => Document::from(false),
            Some(Field::Null) => Document::Null,
            Some(Field::Value(v)) => Document::from(*v),
        };
        row.set_default("available", available)?;
        let undercut = match (
            row.view().available.as_ref(),
            floor,
            row.view().buyout_copper,
            row.view().count,
        ) {
            (Some(true), Some(floor), Some(buyout), Some(count)) if count != 0 => {
                Some(buyout.0 as f64 / count as f64 > floor.0 as f64)
            }
            _ => None,
        };
        row.set("floor_copper", Document::from_typed(&floor)?)?;
        row.set("undercut", Document::from(undercut))?;
    }
    let count = rows.len();
    if data.document().get("listings").is_some() {
        data.set(
            "listings",
            Document::Array(rows.into_iter().map(Payload::into_document).collect()),
        )?;
    }
    let mut doc = data.into_document();
    recount(&mut doc, count)?;
    Payload::decode(doc, "listings")
}
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = args
        .item
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| item_for(s, context))
        .transpose()?;
    let (prices, market_source) = crate::market::market_table(context)?;
    let item_id = item.as_ref().map(|i| i.id.to_string());
    let mut data = listings(
        args.char.as_deref(),
        item_id.as_deref(),
        Some(&prices),
        context,
    )?;
    let mut used = sources(data.document())?;
    used.push(market_source.clone());
    data.set("sources", source_documents(used))?;
    let mut freshness = vec![market_source];
    freshness.extend(listing_sources(&data)?);
    let stale = is_stale(&freshness);
    let lines = crate::human::listings::lines(&listing_rows(&data)?);
    let mut document = data.into_document();
    disclose_item_source(&mut document, item.as_ref(), context)?;
    let mut output = CommandOutput::new(document, lines);
    output.stale = stale;
    Ok(output)
}
