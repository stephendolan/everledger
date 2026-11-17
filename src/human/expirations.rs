use crate::{
    human::{copper_money, integer, number},
    market::Price,
};
pub fn repost(price: &Price) -> String {
    format!(
        "  repost input: floor {} | listed {} | sales/day {}",
        copper_money(price.floor_copper),
        integer(price.quantity),
        number(price.sales_per_day)
    )
}
