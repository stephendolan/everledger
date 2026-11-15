#![forbid(unsafe_code)]
fn main() {
    let args = everledger::cli::parse_env().unwrap_or_else(|error| error.exit());
    let config = everledger::config::Config::from_env();
    let system = everledger::clock::SystemClock::default();
    #[cfg(feature = "parity-seams")]
    let virtual_clock = match everledger::clock::VirtualClock::from_env() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(e.code as i32)
        }
    };
    #[cfg(feature = "parity-seams")]
    let clock: &dyn everledger::clock::Clock = virtual_clock
        .as_ref()
        .map_or(&system as &dyn everledger::clock::Clock, |c| c);
    #[cfg(not(feature = "parity-seams"))]
    let clock: &dyn everledger::clock::Clock = &system;
    let http = everledger::http::UreqHttp;
    let catalogue = everledger::catalogue::CatalogueStore::default();
    let diagnostics = everledger::diagnostics::WriterDiagnostics::new(std::io::stderr());
    let context = everledger::Context {
        diagnostics: &diagnostics,
        clock,
        http: &http,
        config: &config,
        catalogue: &catalogue,
    };
    let code = everledger::cli::execute(&args, &context, &mut std::io::stdout().lock());
    std::process::exit(code as i32);
}
