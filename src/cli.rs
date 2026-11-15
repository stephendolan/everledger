use crate::{
    Context, ExitCode, Failure, Result,
    commands::{self, CommandOutput},
    document::{JsonMode, dumps},
};
use clap::ArgMatches;
use std::io::Write;
#[path = "cli_surface.rs"]
mod surface;
pub use surface::parser;

pub fn dispatch(args: &ArgMatches, context: &Context<'_>) -> Result<CommandOutput> {
    let (name, args) = args.subcommand().expect("required subcommand");
    match name {
        "price" => commands::price::run(args, context),
        "recipe" => commands::recipe::run(args, context),
        "bridge" => commands::bridge::run(args, context),
        "margins" => commands::margins::run(args, context),
        "vendors" => commands::vendors::run(args, context),
        "char" => commands::char::run(args, context),
        "listings" => commands::listings::run(args, context),
        "positions" => commands::positions::run(args, context),
        "sales" => commands::sales::run(args, context),
        "purchases" => commands::purchases::run(args, context),
        "expirations" => commands::expirations::run(args, context),
        "net" => commands::net::run(args, context),
        "events" => commands::events::run(args, context),
        "status" => commands::status::run(args, context),
        "refresh" => commands::refresh::run(args, context),
        "verify" => commands::verify::run(args, context),
        _ => unreachable!("clap validates registered commands"),
    }
}
fn validate(args: &ArgMatches) -> Result<()> {
    if let Some(("events", args)) = args.subcommand() {
        if args
            .get_one::<String>("cursor")
            .is_some_and(|s| s.is_empty())
        {
            return Err(Failure::usage(
                "empty --cursor; pass next_cursor from the previous page",
            ));
        }
        if args
            .get_one::<crate::types::CliInteger>("limit")
            .is_some_and(|n| n.is_negative())
        {
            return Err(Failure::usage("--limit must be nonnegative"));
        }
    }
    Ok(())
}
pub fn execute(
    args: &ArgMatches,
    context: &Context<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    if args.subcommand_name() == Some("verify") {
        let _ = writeln!(stderr, "deprecated verify; use: everledger status");
    }
    let result = validate(args)
        .and_then(|()| dispatch(args, context))
        .and_then(|output| {
            for warning in &output.warnings {
                writeln!(stderr, "{warning}")?;
            }
            let (_, sub) = args.subcommand().unwrap();
            let rendered = crate::human::emit(
                &output.data,
                &output.lines,
                sub.get_flag("json"),
                output.stale,
                &output.source_families,
                context.clock,
            )?;
            stdout.write_all(rendered.as_bytes())?;
            Ok(output.code)
        });
    match result {
        Ok(code) => code,
        Err(error) => {
            let _ = write_failure(&error, stderr);
            error.code
        }
    }
}
pub fn write_failure(error: &Failure, stderr: &mut dyn Write) -> Result<()> {
    writeln!(stderr, "{}", error.message)?;
    if let Some(response) = error
        .body
        .as_ref()
        .and_then(|b| b.get("response"))
        .filter(|v| v.truthy())
    {
        writeln!(stderr, "{}", dumps(response, JsonMode::Error))?;
    }
    Ok(())
}
