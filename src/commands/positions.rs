#[derive(clap::Args, Debug, Clone)]
#[command(args_override_self = true)]
pub struct Args {
    #[arg(long = "char", allow_negative_numbers = true)]
    pub char: Option<String>,
    #[arg(long = "item", allow_negative_numbers = true)]
    pub item: Option<String>,
    #[command(flatten)]
    pub output: crate::cli::OutputArgs,
}
use crate::{
    Context, Copper, Failure, Result,
    catalogue::{self, Item, ItemRef, RecordedItem, item_matches},
    commands::{CommandOutput, listings},
    document::Document,
    http::Payload,
    journal::{self, Character, Selectors},
    provenance::{Field, is_stale, sources},
    roster::{character_path, roster},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Characters {
    #[serde(default)]
    characters: Vec<Character>,
}
#[derive(Deserialize)]
struct InventoryItem {
    item_id: i64,
    count: i64,
    name: Option<String>,
    item: Option<RecordedItem>,
}
impl InventoryItem {
    fn item(&self) -> ItemRef<'_> {
        ItemRef {
            id: match self.item.as_ref().map(|i| &i.id) {
                None | Some(Field::Missing) => Some(self.item_id),
                Some(Field::Null) => None,
                Some(Field::Value(id)) => Some(*id),
            },
            name: match self.item.as_ref().map(|i| &i.name) {
                None | Some(Field::Missing) => self.name.as_deref(),
                Some(Field::Null) => None,
                Some(Field::Value(name)) => Some(name),
            },
            id_basis: self.item.as_ref().and_then(|i| i.id_basis.as_deref()),
        }
    }
}
#[derive(Deserialize, Default)]
struct Section {
    #[serde(default)]
    available: Field<bool>,
    #[serde(default)]
    items: Vec<InventoryItem>,
}
#[derive(Deserialize)]
struct Inventory {
    #[serde(default)]
    bags: Section,
    #[serde(default)]
    bank: Section,
    #[serde(default)]
    equipped: Section,
    #[serde(default)]
    mail: Section,
}
#[derive(Serialize)]
pub struct Location {
    pub character: Option<String>,
    pub location: &'static str,
    pub count: i64,
}
#[derive(Serialize)]
pub struct Availability {
    pub character: Option<String>,
    pub location: &'static str,
    pub available: Option<bool>,
}
#[derive(Serialize, Deserialize)]
pub struct PositionItem {
    #[serde(deserialize_with = "catalogue::nullable")]
    pub id: Option<i64>,
    #[serde(deserialize_with = "catalogue::nullable")]
    pub name: Option<String>,
}
impl PositionItem {
    fn matches(&self, row: ItemRef<'_>) -> bool {
        if let Some(id) = self.id {
            return item_matches(
                row,
                &Item {
                    id,
                    name: self.name.clone(),
                },
            );
        }
        row.id.is_none()
            && row
                .name
                .filter(|s| !s.is_empty())
                .zip(self.name.as_deref().filter(|s| !s.is_empty()))
                .is_some_and(|(a, b)| {
                    crate::primitives::casefold(a) == crate::primitives::casefold(b)
                })
    }
}
pub struct Position {
    pub item: Payload<PositionItem>,
    pub held: i64,
    pub locations: Vec<Location>,
    pub listed: i64,
    pub floor_copper: Option<Copper>,
    pub value_copper: Option<Copper>,
    pub purchases: Vec<journal::EventRow>,
    pub match_basis: Vec<&'static str>,
}
impl Position {
    fn new(item: Payload<PositionItem>) -> Self {
        Self {
            item,
            held: 0,
            locations: vec![],
            listed: 0,
            floor_copper: None,
            value_copper: None,
            purchases: vec![],
            match_basis: vec![],
        }
    }
    fn document(&self) -> Result<Document> {
        Ok(Document::object([
            ("item", self.item.document().clone()),
            ("held", Document::from(self.held)),
            ("locations", Document::from_typed(&self.locations)?),
            ("listed", Document::from(self.listed)),
            ("floor_copper", Document::from_typed(&self.floor_copper)?),
            ("value_copper", Document::from_typed(&self.value_copper)?),
            ("basis_copper", Document::Null),
            (
                "basis",
                Document::from("unknown: no supported lot assignments"),
            ),
            (
                "purchases",
                Document::Array(
                    self.purchases
                        .iter()
                        .map(|e| e.document().clone())
                        .collect(),
                ),
            ),
            ("match_basis", Document::from_typed(&self.match_basis)?),
        ]))
    }
}
fn position(rows: &mut Vec<Position>, item: Payload<PositionItem>) -> &mut Position {
    let index = rows
        .iter()
        .position(|r| r.item.view().id == item.view().id)
        .unwrap_or_else(|| {
            rows.push(Position::new(item));
            rows.len() - 1
        });
    &mut rows[index]
}
pub fn run(args: &Args, context: &Context<'_>) -> Result<CommandOutput> {
    let item = args
        .item
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| catalogue::item_for(s, context))
        .transpose()?;
    let (prices, market_source) = crate::market::market_table(context)?;
    let chars: Payload<Characters> = roster(context.http, context.config, "/characters", &[])?;
    let params = journal::event_params(
        Selectors {
            character: args.char.as_deref(),
            since: None,
            until: None,
            venue: None,
            event_type: Some("purchase"),
            after: None,
            limit: None,
            default_since: Some("7d"),
        },
        context,
    )?;
    let entries = if args.char.as_deref().is_some_and(|s| !s.is_empty()) {
        vec![Character {
            name: params.view.char.clone().map_or(Field::Null, Field::Value),
            short_name: None,
        }]
    } else {
        chars.view().characters.clone()
    };
    let journal = journal::item_events(params, item.as_ref(), context)?;
    let item_id = item.as_ref().map(|i| i.id.to_string());
    let current = listings::listings(
        args.char.as_deref(),
        item_id.as_deref().or(args.item.as_deref()),
        None,
        context,
    )?;
    let mut rows = vec![];
    let mut used = sources(chars.document())?;
    used.extend(sources(journal.document())?);
    used.extend(sources(current.document())?);
    used.push(market_source.clone());
    let mut freshness = sources(chars.document())?;
    freshness.extend(listings::listing_sources(&current)?);
    freshness.push(market_source);
    let mut availability = vec![];
    for ch in entries {
        let name = ch
            .name
            .as_ref()
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .or(ch.short_name.as_deref());
        let inventory_name = name.unwrap_or("None");
        let inv: Payload<Inventory> = roster(
            context.http,
            context.config,
            &(character_path(inventory_name) + "/inventory"),
            &[],
        )?;
        used.extend(sources(inv.document())?);
        freshness.extend(sources(inv.document())?);
        for (location, section) in [
            ("bags", &inv.view().bags),
            ("bank", &inv.view().bank),
            ("equipped", &inv.view().equipped),
            ("mail", &inv.view().mail),
        ] {
            availability.push(Availability {
                character: name.map(str::to_owned),
                location,
                available: match section.available {
                    Field::Missing => Some(false),
                    Field::Null => None,
                    Field::Value(v) => Some(v),
                },
            });
            for r in &section.items {
                if item
                    .as_ref()
                    .is_some_and(|item| !item_matches(r.item(), item))
                {
                    continue;
                }
                let held_item = PositionItem {
                    id: Some(r.item_id),
                    name: r.name.clone(),
                };
                let item =
                    Payload::decode(Document::from_typed(&held_item)?, "inventory position item")?;
                let row = position(&mut rows, item);
                row.held = row
                    .held
                    .checked_add(r.count)
                    .ok_or_else(|| Failure::error("held count out of range"))?;
                row.locations.push(Location {
                    character: name.map(str::to_owned),
                    location,
                    count: r.count,
                });
            }
        }
    }
    for r in listings::listing_rows(&current)? {
        if item
            .as_ref()
            .is_some_and(|item| !item_matches(r.view().item(), item))
        {
            continue;
        }
        let item = r.view().item();
        let item = PositionItem {
            id: item.id,
            name: item.name.map(str::to_owned),
        };
        let item = Payload::decode(Document::from_typed(&item)?, "listing position item")?;
        let row = position(&mut rows, item);
        row.listed = row
            .listed
            .checked_add(r.view().count.unwrap_or(0))
            .ok_or_else(|| Failure::error("listed count out of range"))?;
    }
    let purchases = journal.rows()?;
    for row in &mut rows {
        row.floor_copper = row
            .item
            .view()
            .id
            .and_then(|id| prices.get(&id))
            .and_then(|p| p.floor_copper);
        row.value_copper = row
            .floor_copper
            .map(|v| {
                v.0.checked_mul(row.held)
                    .map(Copper)
                    .ok_or_else(|| Failure::error("held value out of range"))
            })
            .transpose()?;
        row.purchases = purchases
            .iter()
            .filter(|e| row.item.view().matches(e.view().item()))
            .cloned()
            .collect();
        row.match_basis = row
            .purchases
            .iter()
            .map(|e| journal::basis(e.view().item()))
            .collect();
        row.match_basis.sort_unstable();
        row.match_basis.dedup();
    }
    let complete = availability.iter().all(|a| a.available == Some(true));
    let stale = is_stale(&freshness);
    let lines = crate::human::positions::lines(&rows, complete);
    let mut document = Document::object([
        (
            "positions",
            Document::Array(
                rows.iter()
                    .map(Position::document)
                    .collect::<Result<Vec<_>>>()?,
            ),
        ),
        (
            "inventory_availability",
            Document::from_typed(&availability)?,
        ),
        ("complete", Document::from(complete)),
        ("stale", Document::from(stale)),
        (
            "journal_available",
            Document::from(journal.view().available),
        ),
        (
            "journal_state",
            Document::from(journal.view().state.clone()),
        ),
        ("sources", journal::source_documents(used)),
    ]);
    catalogue::disclose_item_source(&mut document, item.as_ref(), context)?;
    let mut output = CommandOutput::new(document, lines);
    output.stale = stale;
    Ok(output)
}
