#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(allow_negative_numbers = true)]
    pub name: Option<String>,
    #[arg(allow_negative_numbers = true)]
    pub profession: Option<String>,
    #[arg(long="skill",value_parser=crate::cli::parse_integer,allow_negative_numbers=true)]
    pub skill: Option<i64>,
    #[arg(long = "json", help = "one JSON document using API field names")]
    pub json: bool,
}
use crate::{
    Context, Failure, Result, catalogue, commands::CommandOutput, http::Payload, read_roster,
};
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    if args.profession.as_deref() == Some("auctions") {
        return Err(Failure::usage(format!(
            "Use: everledger listings --char \"{}\"",
            args.name.as_deref().unwrap_or("None")
        )));
    }
    let (mut data, lines, mut used) = if let Some(name) = &args.name {
        let snapshot = read_roster::character(context, name)?;
        let mut used = crate::provenance::sources(snapshot.payload.document())?;
        let cat = if args.profession.is_some() {
            let c = catalogue::load_catalogue(context)?;
            used.push(catalogue::catalogue_source(c, context.clock)?);
            Some(c)
        } else {
            None
        };
        let lines =
            crate::human::char::character(&snapshot, cat, args.profession.as_deref(), args.skill)?;
        (snapshot.payload.into_document(), lines, used)
    } else {
        #[derive(serde::Deserialize)]
        struct List {
            #[serde(default)]
            characters: Vec<read_roster::Character>,
        }
        let p: Payload<List> =
            crate::roster::roster(context.http, context.config, "/characters", &[])?;
        let lines = crate::human::char::list(&p.view().characters);
        let used = crate::provenance::sources(p.document())?;
        (p.into_document(), lines, used)
    };
    data.set(
        "sources",
        crate::document::Document::Array(used.drain(..).map(|s| s.into_document()).collect()),
    )?;
    Ok(CommandOutput::new(data, lines))
}
