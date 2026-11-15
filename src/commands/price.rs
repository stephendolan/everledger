#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers=true,required=true,num_args=1..)]
    pub item: Vec<String>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, Copper, Result,
    catalogue::nullable,
    commands::CommandOutput,
    document::Document,
    http::Payload,
    primitives::{Scalar, rounded_copper},
    vendors::VendorAnswer,
};
use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct MarketItem {
    #[serde(deserialize_with = "nullable")]
    pub name: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct Latest {
    #[serde(rename = "observedAt", deserialize_with = "nullable")]
    pub observed_at: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub median: Option<f64>,
    #[serde(rename = "minBuyout", deserialize_with = "nullable")]
    pub min_buyout: Option<f64>,
    #[serde(rename = "median7d", deserialize_with = "nullable")]
    pub median_7d: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub quantity: Option<Scalar>,
    #[serde(deserialize_with = "nullable")]
    pub auctions: Option<Scalar>,
}
#[derive(Debug, Deserialize)]
pub struct Sales {
    #[serde(rename = "estimatedPerDay", deserialize_with = "nullable")]
    pub estimated_per_day: Option<Scalar>,
    #[serde(rename = "coveredHours", deserialize_with = "nullable")]
    pub covered_hours: Option<Scalar>,
}
#[derive(Debug, Deserialize)]
pub struct MarketQuote {
    pub item: MarketItem,
    pub latest: Latest,
    pub sales: Sales,
}
pub struct RoundedQuote {
    pub median: Option<Copper>,
    pub min_buyout: Option<Copper>,
    pub median_7d: Option<Copper>,
}
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = crate::catalogue::item_for(&args.item.join(" "), context)?;
    let response = context.http.request(&crate::http::Request::get(format!(
        "{}/v1/prices/{}/{}",
        context.config.ahledger_url, context.config.market, item.id
    )))?;
    let data: Payload<MarketQuote> =
        Payload::decode(crate::http::decode_body(&response.body)?, "AHledger quote")?;
    let v = data.view();
    let latest = &v.latest;
    let rounded = RoundedQuote {
        median: latest.median.map(rounded_copper).transpose()?,
        min_buyout: latest.min_buyout.map(rounded_copper).transpose()?,
        median_7d: latest.median_7d.map(rounded_copper).transpose()?,
    };
    let mut source = crate::provenance::source(
        &format!("AHledger {}", context.config.market),
        latest.observed_at.as_deref(),
        context.clock,
    )?;
    source.set(
        "stale",
        crate::time::age(latest.observed_at.as_deref(), context.clock).is_none_or(|a| a > 900),
    )?;
    let field = |key| {
        data.document()
            .get(key)
            .cloned()
            .ok_or_else(|| crate::Failure::shape("AHledger quote", format!("missing {key}")))
    };
    let mut quote = Document::object([
        ("item", field("item")?),
        (
            "latest",
            Document::object([
                ("observed_at", Document::from(latest.observed_at.clone())),
                ("median_copper", Document::from_typed(&rounded.median)?),
                (
                    "min_buyout_copper",
                    Document::from_typed(&rounded.min_buyout)?,
                ),
                (
                    "median_7d_copper",
                    Document::from_typed(&rounded.median_7d)?,
                ),
                ("quantity", Document::from_typed(&latest.quantity)?),
                ("auctions", Document::from_typed(&latest.auctions)?),
            ]),
        ),
        ("sales", field("sales")?),
        ("sources", Document::Array(vec![source.into_document()])),
    ]);
    let vendors = crate::vendors::lookup(item.id, false, context)?;
    match &vendors {
        VendorAnswer::Ninja(a) => {
            quote.set("vendors", a.sellers_document()?)?;
            quote.set("vendor_reason", Document::from(a.reason()))?;
            let mut sources = crate::provenance::sources(&quote)?;
            sources.extend(crate::vendors::sources(a)?);
            quote.set(
                "sources",
                Document::Array(sources.into_iter().map(|s| s.into_document()).collect()),
            )?;
        }
    }
    let lines = crate::human::price::render(item.id, v, &rounded, &vendors);
    crate::catalogue::disclose_item_source(&mut quote, Some(&item), context)?;
    let code = crate::provenance::stale_code(&crate::provenance::sources(&quote)?);
    let mut out = CommandOutput::new(quote, lines);
    out.code = code;
    Ok(out)
}
