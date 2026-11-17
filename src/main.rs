#![forbid(unsafe_code)]
fn main() {
    let diagnostics = everledger::diagnostics::WriterDiagnostics::new(std::io::stderr());
    let code = match run(&diagnostics) {
        Ok(code) => code,
        Err(error) => {
            let _ = everledger::cli::write_failure(&error, &diagnostics);
            error.code
        }
    };
    std::process::exit(code as i32);
}
fn run(
    diagnostics: &dyn everledger::diagnostics::Diagnostics,
) -> everledger::Result<everledger::ExitCode> {
    let config = everledger::config::Config::from_env()?;
    let args = everledger::cli::parse_env().unwrap_or_else(|error| error.exit());
    let system = everledger::clock::SystemClock::default();
    #[cfg(feature = "parity-seams")]
    let virtual_clock = everledger::clock::VirtualClock::from_env()?;
    #[cfg(feature = "parity-seams")]
    let clock: &dyn everledger::clock::Clock = virtual_clock
        .as_ref()
        .map_or(&system as &dyn everledger::clock::Clock, |c| c);
    #[cfg(not(feature = "parity-seams"))]
    let clock: &dyn everledger::clock::Clock = &system;
    let http = everledger::http::UreqHttp;
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let context = everledger::Context {
        diagnostics,
        clock,
        http: &http,
        config: &config,
        catalogue: &catalogue,
    };
    Ok(everledger::cli::execute(
        &args,
        &context,
        &mut std::io::stdout().lock(),
    ))
}
