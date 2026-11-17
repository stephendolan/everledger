#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers=true,required=true,num_args=1..)]
    pub item: Vec<String>,
    #[arg(long="skill",value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub skill: Option<i64>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, Failure, Result, catalogue,
    commands::CommandOutput,
    document::Document,
    http::Payload,
    read_roster::{Characters, Knowledge, known_ids},
};
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let cat = catalogue::load_catalogue(context)?;
    let query = crate::primitives::lower(&args.item.join(" "));
    let hits = catalogue::recipes(cat)?
        .into_iter()
        .filter(|r| crate::primitives::lower(&r.recipe.name).contains(&query))
        .collect::<Vec<_>>();
    let exact = hits
        .iter()
        .any(|r| crate::primitives::lower(&r.recipe.name) == query);
    let hits = hits
        .into_iter()
        .filter(|r| !exact || crate::primitives::lower(&r.recipe.name) == query)
        .take(8)
        .collect::<Vec<_>>();
    if hits.is_empty() {
        return Err(Failure::error("no catalogue match"));
    }
    let mut used = vec![catalogue::catalogue_source(cat, context.clock)?];
    let mut rows = hits
        .iter()
        .map(|r| {
            let mut d = r.document.clone();
            d.set("profession", r.profession)?;
            Ok(d)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut lines = hits
        .iter()
        .map(|r| crate::human::recipe::recipe(r.profession, r.recipe, args.skill))
        .collect::<Vec<_>>();
    let roster: Result<Payload<Characters>> =
        crate::roster::roster(context.http, context.config, "/characters", &[]);
    match roster {
        Ok(chars) => {
            used.extend(crate::provenance::sources(chars.document())?);
            for (hit, row) in hits.iter().zip(&mut rows) {
                let mut known = vec![];
                let mut unavailable = vec![];
                for ch in &chars.view().characters {
                    let name = ch
                        .name
                        .as_deref()
                        .filter(|n| !n.is_empty())
                        .or(ch.short_name.as_deref());
                    match known_ids(ch, hit.profession) {
                        Knowledge::Unavailable => unavailable.push(name),
                        Knowledge::Available(ids) => {
                            if ids.contains(&hit.recipe.spell_id) {
                                known.push(name);
                            }
                        }
                    }
                }
                row.set("known_by", Document::from_typed(&known)?)?;
                row.set(
                    "recipe_data_unavailable",
                    Document::from_typed(&unavailable)?,
                )?;
                lines.push(crate::human::recipe::knowledge(&known, &unavailable)?);
            }
        }
        Err(e) => {
            for row in &mut rows {
                row.set("known_by", Document::Null)?;
            }
            lines.push(crate::human::recipe::unchecked(&e));
        }
    }
    let stale = crate::provenance::is_stale(&used);
    let mut output = CommandOutput::new(
        Document::object([
            ("recipes", Document::Array(rows)),
            (
                "sources",
                Document::Array(used.into_iter().map(|s| s.into_document()).collect()),
            ),
        ]),
        lines,
    );
    output.stale = stale;
    Ok(output)
}
