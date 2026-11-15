use crate::{Copper, human::copper_money};
pub fn total(proceeds: Option<Copper>) -> String {
    format!("total proceeds {}", copper_money(proceeds))
}
