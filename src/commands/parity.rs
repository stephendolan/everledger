use crate::{
    Context, Copper, Failure, Result,
    catalogue::{self, RecipeItem},
    commands::CommandOutput,
    document::Document,
    market::{self, PriceTable, checked_copper, input_cost, net_after_auction_cut},
};
use serde::{Deserialize, Serialize};

#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long, help = "Include conversions with no positive spread")]
    pub all: bool,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}

#[derive(Deserialize)]
struct ItemUse {
    name: String,
    source_url: String,
    input: RecipeItem,
    output: RecipeItem,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConversionSource<'a> {
    Catalogue { profession: &'a str, spell_id: i64 },
    ItemUse { url: &'a str },
}

#[derive(Serialize)]
struct Conversion<'a> {
    name: &'a str,
    inputs: &'a [RecipeItem],
    output: &'a RecipeItem,
    source: ConversionSource<'a>,
}

#[derive(Serialize)]
struct Spread {
    input_cost_copper: Copper,
    output_value_copper: Copper,
    spread_copper: Copper,
    spread_percent: f64,
}

impl Spread {
    fn new(cost: Copper, value: Copper) -> Result<Self> {
        let spread = checked_copper(value.0.checked_sub(cost.0))?;
        Ok(Self {
            input_cost_copper: cost,
            output_value_copper: value,
            spread_copper: spread,
            spread_percent: 100.0 * spread.0 as f64 / cost.0 as f64,
        })
    }

    fn delta(&self) -> String {
        format!("{:+} ({:+.1}%)", self.spread_copper.0, self.spread_percent)
    }

    fn describe(&self) -> String {
        format!(
            "{} -> {} [{}]",
            self.input_cost_copper.0,
            self.output_value_copper.0,
            self.delta()
        )
    }
}

#[derive(Serialize)]
struct Row<'a> {
    #[serde(flatten)]
    conversion: Conversion<'a>,
    floor_realized: Spread,
    floor: Spread,
    median: Spread,
    median_after_cut: Spread,
    output_listed_units: Option<i64>,
    output_sales_per_day: Option<f64>,
    weighted_spread_copper_per_day: Option<f64>,
    verdict: &'static str,
}

fn quote<'a>(conversion: Conversion<'a>, prices: &PriceTable) -> Result<Option<Row<'a>>> {
    let supported = |item: &RecipeItem| item.qty > 0 && item.item_id.is_some_and(|id| id > 0);
    if conversion.inputs.is_empty()
        || !conversion.inputs.iter().all(supported)
        || !supported(conversion.output)
    {
        return Ok(None);
    }
    let Some(price) = conversion.output.item_id.and_then(|id| prices.get(&id)) else {
        return Ok(None);
    };
    let priced = |p: Option<Copper>| p.filter(|v| v.0 > 0);
    let (Some(floor), Some(median)) = (priced(price.floor_copper), priced(price.median_copper))
    else {
        return Ok(None);
    };
    let Some(floor_cost) = input_cost(conversion.inputs, prices, |p| priced(p.floor_copper))?
    else {
        return Ok(None);
    };
    let Some(median_cost) = input_cost(conversion.inputs, prices, |p| priced(p.median_copper))?
    else {
        return Ok(None);
    };
    let qty = conversion.output.qty;
    let gross = |unit: Copper| checked_copper(unit.0.checked_mul(qty));
    let posted = Copper((floor.0 - 1).max(1));
    let floor_realized = Spread::new(floor_cost, net_after_auction_cut(gross(posted)?)?)?;
    let floor = Spread::new(floor_cost, gross(floor)?)?;
    let median = Spread::new(median_cost, gross(median)?)?;
    let median_after_cut = Spread::new(
        median_cost,
        net_after_auction_cut(median.output_value_copper)?,
    )?;
    let verdict = match (
        floor_realized.spread_copper.0,
        median_after_cut.spread_copper.0,
    ) {
        (a, _) if a > 0 => "positive at floors",
        (_, b) if b > 0 => "positive only at medians",
        (a, b) if a < 0 && b < 0 => "negative both",
        _ => "break-even on at least one basis",
    };
    let sales = price.sales_per_day.filter(|s| s.is_finite() && *s >= 0.0);
    let weighted = sales
        .map(|s| (floor_realized.spread_copper.0 as f64 / qty as f64) * s)
        .filter(|s| s.is_finite());
    Ok(Some(Row {
        conversion,
        floor_realized,
        floor,
        median,
        median_after_cut,
        output_listed_units: price.quantity,
        output_sales_per_day: sales,
        weighted_spread_copper_per_day: weighted,
        verdict,
    }))
}

fn render(rows: &[Row<'_>]) -> Vec<String> {
    let mut lines =
        vec!["Prices from AHledger (ahledger.com). Values in copper per conversion.".into()];
    if rows.is_empty() {
        lines.push("no matching priced conversions".into());
        return lines;
    }
    let names: Vec<_> = rows
        .iter()
        .map(|row| {
            let source = match row.conversion.source {
                ConversionSource::Catalogue { profession, .. } => profession,
                ConversionSource::ItemUse { .. } => "item use",
            };
            format!("{} ({source})", row.conversion.name)
        })
        .collect();
    let width = names
        .iter()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0)
        .max(10);
    lines.push(format!(
        "{:<width$} | {:>18} | {:>31} | {:>31} | {:>8} | {:>9} | Verdict",
        "Conversion",
        "Realized spread",
        "Floor cost -> value [spread]",
        "Median cost -> net [spread]",
        "Listed",
        "Sales/day"
    ));
    for (row, name) in rows.iter().zip(names) {
        let listed = row
            .output_listed_units
            .map_or_else(|| "unknown".into(), |n| n.to_string());
        let sales = row
            .output_sales_per_day
            .map_or_else(|| "unknown".into(), |n| format!("{n:.1}"));
        lines.push(format!(
            "{name:<width$} | {:>18} | {:>31} | {:>31} | {listed:>8} | {sales:>9} | {}",
            row.floor_realized.delta(),
            row.floor.describe(),
            row.median_after_cut.describe(),
            row.verdict,
        ));
    }
    lines
}

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = catalogue::load_catalogue(context)?;
    let cat_source = catalogue::catalogue_source(cat, context.clock)?;
    let item_uses: Vec<ItemUse> =
        serde_json::from_str(include_str!("../../data/item-use-conversions.json"))
            .map_err(|e| Failure::shape("item-use conversions", e))?;

    let (prices, mut market_source) = market::market_table(context)?;
    market_source.set("detail", "Prices from AHledger (https://ahledger.com)")?;
    let sources = [cat_source, market_source];
    let recipes = catalogue::recipes(cat)?.into_iter().map(|r| Conversion {
        name: &r.recipe.name,
        inputs: &r.recipe.reagents,
        output: &r.recipe.result,
        source: ConversionSource::Catalogue {
            profession: r.profession,
            spell_id: r.recipe.spell_id,
        },
    });
    let uses = item_uses.iter().map(|r| Conversion {
        name: &r.name,
        inputs: std::slice::from_ref(&r.input),
        output: &r.output,
        source: ConversionSource::ItemUse { url: &r.source_url },
    });

    let mut rows = Vec::new();
    for conversion in recipes.chain(uses) {
        if let Some(row) = quote(conversion, &prices)?
            && (args.all
                || row.floor_realized.spread_copper.0 > 0
                || row.median_after_cut.spread_copper.0 > 0)
        {
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| {
        let score = |row: &Row<'_>| {
            row.weighted_spread_copper_per_day
                .unwrap_or(f64::NEG_INFINITY)
        };
        score(b)
            .total_cmp(&score(a))
            .then_with(|| {
                b.floor_realized
                    .spread_copper
                    .cmp(&a.floor_realized.spread_copper)
            })
            .then_with(|| a.conversion.name.cmp(b.conversion.name))
    });
    let stale = crate::provenance::is_stale(&sources);
    let lines = render(&rows);
    let mut out = CommandOutput::new(
        Document::object([
            ("parities", Document::from_typed(&rows)?),
            (
                "sources",
                Document::Array(sources.into_iter().map(|s| s.into_document()).collect()),
            ),
        ]),
        lines,
    );
    out.stale = stale;
    Ok(out)
}
