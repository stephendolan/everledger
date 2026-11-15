use crate::Result;
use std::{cell::RefCell, io::Write};
pub trait Diagnostics {
    fn write(&self, text: &str) -> Result<()>;
    fn warn(&self, message: &str) -> Result<()> {
        self.write(&format!("{message}\n"))
    }
}
pub struct WriterDiagnostics<W: Write> {
    writer: RefCell<W>,
}
impl<W: Write> WriterDiagnostics<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: RefCell::new(writer),
        }
    }
    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
}
impl<W: Write> Diagnostics for WriterDiagnostics<W> {
    fn write(&self, text: &str) -> Result<()> {
        self.writer
            .try_borrow_mut()
            .map_err(|_| crate::Failure::error("diagnostics writer is already in use"))?
            .write_all(text.as_bytes())?;
        Ok(())
    }
}
impl Default for WriterDiagnostics<Vec<u8>> {
    fn default() -> Self {
        Self::new(vec![])
    }
}
impl WriterDiagnostics<Vec<u8>> {
    pub fn contents(&self) -> Vec<u8> {
        self.writer.borrow().clone()
    }
}
