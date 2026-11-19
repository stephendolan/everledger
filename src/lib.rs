#![forbid(unsafe_code)]

pub mod catalogue;
pub mod catalogue_build;
pub mod catalogue_check;
pub mod cli;
pub mod clock;
pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod document;
pub mod foreverdb;
pub mod http;
pub mod human;
pub mod journal;
pub mod market;
pub mod primitives;
pub mod provenance;
pub mod read_roster;
pub mod roster;
pub mod time;
pub mod types;
pub mod vendors;

pub use types::{Copper, EpochSeconds, ExitCode, Failure, Result};

pub struct Context<'a> {
    pub clock: &'a dyn clock::Clock,
    pub diagnostics: &'a dyn diagnostics::Diagnostics,
    pub http: &'a dyn http::Http,
    pub config: &'a config::Config,
    pub catalogue: &'a catalogue::CatalogueStore,
}
