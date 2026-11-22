use crate::{ExitCode, document::Document, human::SourceFamilies};
pub mod bridge;
pub mod char;
pub mod events;
pub mod expirations;
pub mod guide;
pub mod listings;
pub mod margins;
pub mod net;
pub mod parity;
pub mod positions;
pub mod price;
pub mod purchases;
pub mod recipe;
pub mod refresh;
pub mod sales;
pub mod status;
pub mod vendors;

pub struct CommandOutput {
    pub data: Document,
    pub lines: Vec<String>,
    pub code: ExitCode,
    pub stale: bool,
    pub source_families: SourceFamilies,
}
impl CommandOutput {
    pub fn new(data: Document, lines: Vec<String>) -> Self {
        Self {
            data,
            lines,
            code: ExitCode::Answer,
            stale: false,
            source_families: vec![],
        }
    }
    pub fn exit_code(&self) -> ExitCode {
        if self.stale {
            ExitCode::Stale
        } else {
            self.code
        }
    }
}
