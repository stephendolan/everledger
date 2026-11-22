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
    market::{checked_copper, input_cost, net_after_auction_cut},
    read_roster::{self, Knowledge},
};
pub struct Margin<'a> {
    pub recipe: &'a catalogue::Recipe,
    pub profession: &'a str,
    pub color: Option<Color>,
    pub margin: Copper,
    pub cost: Copper,
    pub sells: Copper,
    pub sales: Option<f64>,
    pub reagents: Vec<Document>,
}
pub struct CraftQuote {
    pub margin: Copper,
    pub cost: Copper,
    pub sells: Copper,
    pub sales: Option<f64>,
}
pub fn quote(
    r: &catalogue::Recipe,
    prices: &crate::market::PriceTable,
) -> Result<Option<CraftQuote>> {
    let Some(price) = r.result.item_id.and_then(|id| prices.get(&id)) else {
        return Ok(None);
    };
    let Some(sells) = price.median_copper else {
        return Ok(None);
    };
    let Some(cost) = input_cost(&r.reagents, prices, |p| p.median_copper)? else {
        return Ok(None);
    };
    let gross = checked_copper(sells.0.checked_mul(r.result.qty))?;
    let net = net_after_auction_cut(gross)?;
    Ok(Some(CraftQuote {
        margin: checked_copper(net.0.checked_sub(cost.0))?,
        cost,
        sells,
        sales: price.sales_per_day,
    }))
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
    let (prices, ms) = crate::market::market_table(context)?;
    used.push(ms);
    let all = catalogue::recipes(cat)?;
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
            let Some(CraftQuote {
                margin,
                cost,
                sells,
                sales,
            }) = quote(recipe, &prices)?
            else {
                continue;
            };
            let docs = r
                .document
                .get("reagents")
                .and_then(Document::as_array)
                .ok_or_else(|| Failure::shape("recipe", "missing reagents"))?;
            let reagents = recipe
                .reagents
                .iter()
                .zip(docs)
                .map(|(x, doc)| {
                    let mut d = doc.clone();
                    d.set(
                        "unit_copper",
                        Document::from_typed(
                            &x.item_id
                                .and_then(|id| prices.get(&id))
                                .and_then(|p| p.median_copper),
                        )?,
                    )?;
                    d.set("basis", "AHledger")?;
                    Ok(d)
                })
                .collect::<Result<Vec<_>>>()?;
            rows.push(Margin {
                recipe,
                profession: r.profession,
                color: skill.map(|s| color_at(&recipe.bands, s)),
                margin,
                cost,
                sells,
                sales,
                reagents,
            });
        }
    }
    rows.sort_by(|a, b| {
        let rank = |r: &Margin<'_>| r.color.map(|c| c as u8).unwrap_or(5);
        let score = |r: &Margin<'_>| {
            if r.margin.0 > 0 {
                -(r.margin.0 as f64 * r.sales.unwrap_or(0.0))
            } else {
                -(r.margin.0 as f64)
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| {
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    rows.truncate(15);
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
                ("margin_copper", Document::from(r.margin.0)),
                ("cost_copper", Document::from(r.cost.0)),
                ("sells_copper", Document::from(r.sells.0)),
                ("sales_per_day", Document::from_typed(&r.sales)?),
                ("reagents", Document::Array(r.reagents.clone())),
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
