#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true)]
    pub profession: Option<String>,
    #[arg(long="level",value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub level: Option<i64>,
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Copper, Failure, Result, catalogue,
    commands::{
        CommandOutput,
        bridge::{Color, color_at},
    },
    document::Document,
    market::{Price, PriceTable},
    read_roster::{self, Knowledge},
};
use std::collections::HashMap;

pub struct Margin<'a> {
    pub recipe: &'a catalogue::Recipe,
    pub profession: &'a str,
    pub color: Option<Color>,
    pub planning: PlanningMargin,
    pub reagents: Vec<Document>,
}

#[derive(Default)]
pub struct PlanningMargin {
    pub margin: Option<Copper>,
    pub cost: Option<Copper>,
    pub sell: Option<(Copper, &'static str)>,
    pub sales: Option<f64>,
    pub reason: Option<&'static str>,
}

fn checked_copper(value: i128) -> Result<Copper> {
    i64::try_from(value)
        .map(Copper)
        .map_err(|_| Failure::error("copper out of range"))
}

fn planning_sell(
    output: Option<i64>,
    yield_qty: i64,
    price: Option<&Price>,
    sales: Option<f64>,
) -> std::result::Result<(Copper, &'static str), &'static str> {
    if output.is_none() {
        return Err("missing_output_mapping");
    }
    if yield_qty <= 0 {
        return Err("invalid_output_yield");
    }
    let price = price.ok_or("missing_output_price")?;
    let rate = sales
        .filter(|n| *n > 0.0)
        .ok_or("missing_positive_sales_rate")?;
    let listed = price
        .quantity
        .filter(|n| *n >= 0)
        .ok_or("missing_current_quote")?;
    let (quote, basis) = if listed as f64 > 2.0 * rate {
        (price.floor_copper, "current_floor")
    } else {
        (price.median_copper, "current_median")
    };
    let quote = quote.filter(|p| p.0 > 0).ok_or("missing_current_quote")?;
    let history = price
        .median_7d_copper
        .filter(|p| p.0 > 0)
        .ok_or("missing_7d_median")?;
    Ok(if history.0 < quote.0 {
        (history, "7d_median")
    } else {
        (quote, basis)
    })
}

fn net_proceeds(price: Copper, yield_qty: i64) -> i128 {
    let gross = i128::from(price.0) * i128::from(yield_qty);
    let fraction = (gross % 100) * 95;
    let whole = (gross / 100) * 95 + fraction / 100;
    let remainder = fraction % 100;
    whole + i128::from(remainder > 50 || remainder == 50 && whole % 2 != 0)
}

fn reagent_floor(item: &catalogue::RecipeItem, prices: &PriceTable) -> Option<Copper> {
    item.item_id
        .filter(|id| *id > 0)
        .and_then(|id| prices.get(&id))
        .and_then(|price| price.floor_copper)
        .filter(|floor| floor.0 >= 0)
}

fn planning_margin(
    recipe: &catalogue::Recipe,
    prices: &PriceTable,
    units: &[Option<Copper>],
    ambiguous: bool,
) -> Result<PlanningMargin> {
    if ambiguous {
        return Ok(PlanningMargin {
            reason: Some("ambiguous_recipe_mapping"),
            ..Default::default()
        });
    }
    let mut result = PlanningMargin::default();
    if units.iter().all(Option::is_some) {
        let cost = recipe
            .reagents
            .iter()
            .zip(units.iter().flatten())
            .try_fold(Copper(0), |total, (reagent, unit)| {
                let subtotal = checked_copper(i128::from(reagent.qty) * i128::from(unit.0))?;
                checked_copper(i128::from(total.0) + i128::from(subtotal.0))
            })?;
        result.cost = Some(cost);
    }
    let output = recipe.result.item_id.filter(|id| *id > 0);
    let price = output.and_then(|id| prices.get(&id));
    let sales = price
        .and_then(|p| p.sales_per_day)
        .filter(|n| n.is_finite());
    result.sales = sales;
    match planning_sell(output, recipe.result.qty, price, sales) {
        Err(reason) => result.reason = Some(reason),
        Ok((sell, basis)) => {
            result.sell = Some((sell, basis));
            if let Some(cost) = result.cost {
                result.margin = Some(checked_copper(
                    net_proceeds(sell, recipe.result.qty) - i128::from(cost.0),
                )?);
            } else {
                result.reason = Some(
                    if recipe
                        .reagents
                        .iter()
                        .any(|r| r.item_id.is_none_or(|id| id <= 0))
                    {
                        "missing_reagent_mapping"
                    } else {
                        "missing_reagent_price"
                    },
                );
            }
        }
    }
    Ok(result)
}

pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = catalogue::load_catalogue(context)?;
    let name = args.char.as_ref().filter(|s| !s.is_empty());
    let mut used = vec![catalogue::catalogue_source(cat, context.clock)?];
    let ch = if let Some(n) = name {
        let s = read_roster::character(context, n)?;
        used.extend(crate::provenance::sources(s.document())?);
        Some(s)
    } else {
        None
    };
    let profs = if let Some(p) = args.profession.as_ref().filter(|s| !s.is_empty()) {
        vec![crate::primitives::casefold(p)]
    } else {
        cat.view()
            .professions
            .0
            .iter()
            .map(|(p, _)| p.clone())
            .collect()
    };
    if profs
        .iter()
        .any(|p| cat.view().professions.get(p).is_none())
    {
        return Err(Failure::usage(format!(
            "unknown profession; choose {}",
            cat.view()
                .professions
                .0
                .iter()
                .map(|(p, _)| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let (prices, ms) = crate::market::planning_table(context)?;
    used.push(ms);
    let all = catalogue::recipes(cat)?;
    let mut recipe_counts = HashMap::new();
    for row in &all {
        *recipe_counts.entry(row.recipe.spell_id).or_insert(0) += 1;
    }
    let mut rows = vec![];
    for prof in &profs {
        let mut skill = args.level;
        let mut known = None;
        if let Some(c) = ch.as_ref().map(|s| &s.view().character) {
            match read_roster::known_ids(c, prof) {
                Knowledge::Unavailable => {
                    if args.profession.as_ref().is_some_and(|p| !p.is_empty()) {
                        return Err(Failure::error(format!(
                            "roster has no recipe data for {} {prof}; craftable set unknown",
                            name.map(String::as_str).unwrap_or("")
                        )));
                    }
                    continue;
                }
                Knowledge::Available(ids) => known = Some(ids),
            }
            if skill.is_none() {
                skill = c
                    .professions
                    .iter()
                    .find(|p| {
                        crate::primitives::casefold(&p.name) == *prof
                            && p.skill_available == Some(true)
                    })
                    .and_then(|p| p.skill.as_ref().copied());
                if skill.is_none() {
                    return Err(Failure::usage(format!(
                        "roster skill unavailable for {prof}; pass --level N"
                    )));
                }
            }
        }
        for r in all.iter().filter(|r| r.profession == prof) {
            let recipe = r.recipe;
            if known
                .as_ref()
                .is_some_and(|ids| !ids.contains(&recipe.spell_id))
                || skill.is_some_and(|s| recipe.bands.learn > s)
            {
                continue;
            }
            let ambiguous = recipe_counts[&recipe.spell_id] > 1;
            let units = recipe
                .reagents
                .iter()
                .map(|r| {
                    if ambiguous {
                        None
                    } else {
                        reagent_floor(r, &prices)
                    }
                })
                .collect::<Vec<_>>();
            let planning = planning_margin(recipe, &prices, &units, ambiguous)?;
            let docs = r
                .document
                .get("reagents")
                .and_then(Document::as_array)
                .ok_or_else(|| Failure::shape("recipe", "missing reagents"))?;
            let reagents = docs
                .iter()
                .zip(units)
                .map(|(doc, unit)| {
                    let mut d = doc.clone();
                    d.set("unit_copper", Document::from_typed(&unit)?)?;
                    d.set("basis", "AHledger")?;
                    Ok(d)
                })
                .collect::<Result<Vec<_>>>()?;
            rows.push(Margin {
                recipe,
                profession: r.profession,
                color: skill.map(|s| color_at(&recipe.bands, s)),
                planning,
                reagents,
            });
        }
    }
    rows.sort_by(|a, b| {
        let rank = |r: &Margin<'_>| {
            (
                r.planning.margin.is_none(),
                r.color.map(|c| c as u8).unwrap_or(5),
            )
        };
        let score = |r: &Margin<'_>| {
            let margin = r.planning.margin.map_or(0.0, |m| m.0 as f64);
            if margin > 0.0 {
                -(margin * r.planning.sales.unwrap_or(0.0))
            } else {
                -margin
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    let docs = rows
        .iter()
        .map(|r| {
            Ok(Document::object([
                ("name", Document::from(r.recipe.name.clone())),
                ("profession", Document::from(r.profession)),
                ("spell_id", Document::from(r.recipe.spell_id)),
                (
                    "color",
                    Document::from(r.color.map(crate::human::bridge::color)),
                ),
                ("margin_copper", Document::from_typed(&r.planning.margin)?),
                ("cost_copper", Document::from_typed(&r.planning.cost)?),
                (
                    "sells_copper",
                    Document::from_typed(&r.planning.sell.map(|(c, _)| c))?,
                ),
                ("sales_per_day", Document::from_typed(&r.planning.sales)?),
                ("reagents", Document::Array(r.reagents.clone())),
                (
                    "price_basis",
                    Document::from(r.planning.sell.map(|(_, basis)| basis)),
                ),
                ("unavailable_reason", Document::from(r.planning.reason)),
                ("source_type", Document::from(r.recipe.source_type.clone())),
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    let lines = crate::human::margins::render(&rows);
    let stale = crate::provenance::is_stale(&used);
    let mut out = CommandOutput::new(
        Document::object([
            ("margins", Document::Array(docs)),
            (
                "sources",
                Document::Array(used.into_iter().map(|s| s.into_document()).collect()),
            ),
        ]),
        lines,
    );
    out.stale = stale;
    Ok(out)
}
