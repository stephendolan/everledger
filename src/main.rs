#![forbid(unsafe_code)]
fn main() {
    let args = everledger::cli::parser().get_matches();
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
    let context = everledger::Context {
        clock,
        http: &http,
        config: &config,
        catalogue: &catalogue,
    };
    let code = everledger::cli::execute(
        &args,
        &context,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code as i32);
}
