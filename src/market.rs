use crate::{
    Context, Copper, EpochSeconds, Failure, Result,
    http::Request,
    primitives::rounded_copper,
    provenance::{Source, source},
    time::{UtcInstant, iso},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Price {
    pub median_copper: Option<Copper>,
    pub floor_copper: Option<Copper>,
    pub quantity: Option<f64>,
    pub sales_per_day: Option<f64>,
}
pub type PriceTable = BTreeMap<i64, Price>;
pub fn market_table(context: &Context<'_>) -> Result<(PriceTable, Source)> {
    let raw = context.http.request(&Request::get(format!(
        "{}/v1/pricetable/{}",
        context.config.ahledger_url, context.config.market
    )))?;
    let text =
        std::str::from_utf8(&raw.body).map_err(|e| Failure::shape("AHledger price table", e))?;
    let mut lines = text.lines();
    let head = lines
        .next()
        .ok_or_else(|| Failure::shape("AHledger price table", "missing header"))?
        .split('|')
        .collect::<Vec<_>>();
    let epoch = head
        .get(2)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Failure::shape("AHledger price table", "missing epoch timestamp"))?;
    let timestamp = iso(UtcInstant::from_epoch(EpochSeconds(epoch))?);
    let mut prices = BTreeMap::new();
    for line in lines {
        let cells = line.split(':').collect::<Vec<_>>();
        let number = |i: usize| -> Result<Option<f64>> {
            let s = cells
                .get(i)
                .ok_or_else(|| Failure::shape("AHledger price table", "missing column"))?;
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse()
                    .map(Some)
                    .map_err(|e| Failure::shape("AHledger price table", e))
            }
        };
        let id = cells[0]
            .parse()
            .map_err(|e| Failure::shape("AHledger price table", e))?;
        prices.insert(
            id,
            Price {
                median_copper: number(1)?.map(rounded_copper).transpose()?,
                floor_copper: number(2)?.map(rounded_copper).transpose()?,
                quantity: number(3)?,
                sales_per_day: number(8)?.map(|v| v / 10.0),
            },
        );
    }
    let mut s = source(
        &format!("AHledger {}", context.config.market),
        Some(&timestamp),
        context.clock,
    )?;
    let stale = crate::time::age(Some(&timestamp), context.clock).is_some_and(|s| s > 900);
    s.set("stale", stale)?;
    Ok((prices, s))
}
