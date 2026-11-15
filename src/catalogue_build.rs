use crate::{
    Context, Failure, Result,
    catalogue::{Catalogue, CatalogueData},
    document::{Document, JsonMode, dumps},
    http::{Payload, Request},
    time::iso,
};
use regex::{Captures, Regex};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
pub const PROFESSIONS: [&str; 12] = [
    "alchemy",
    "blacksmithing",
    "enchanting",
    "engineering",
    "leatherworking",
    "tailoring",
    "cooking",
    "first-aid",
    "fishing",
    "mining",
    "herbalism",
    "skinning",
];
#[derive(Deserialize)]
struct Entities {
    entities: BTreeMap<String, String>,
    invalid_refs: BTreeMap<String, String>,
    invalid_points: BTreeSet<u32>,
}
struct Patterns {
    row: Regex,
    bands: Regex,
    recipe: Regex,
    item: Regex,
    cell: Regex,
    count: Regex,
    tag: Regex,
    entity: Regex,
    entities: Entities,
}
static PATTERNS: OnceLock<std::result::Result<Patterns, String>> = OnceLock::new();
impl Patterns {
    fn load() -> Result<&'static Self> {
        PATTERNS.get_or_init(|| {
            let regex=|s| Regex::new(s).map_err(|e|e.to_string());
            Ok(Self {
                row: regex(r#"(?s)<tr role="row"(.*?)</tr>"#)?,
                bands: regex(r#"<span class="o">(\d+)</span>(?:\s*<span class="y">(\d+)</span>)?(?:\s*<span class="g">(\d+)</span>)?(?:\s*<span class="x">(\d+)</span>)?"#)?,
                recipe: regex(r#"(?s)href="/professions/[^/"]+/(\d+)-[^"]*/"[^>]*>(.*?)</a>"#)?,
                item: regex(r#"(?s)href="/items/(\d+)-[^"]*/"[^>]*>(?:<img[^>]*>)?\s*(.*?)</a>(\s*×\s*(\d+))?"#)?,
                cell: regex(r#"(?s)<td[^>]*role="cell"[^>]*>(.*?)</td>"#)?,
                count: regex(r"Recipes \((\d+)\)")?, tag: regex(r"<[^>]+>")?,
                entity: regex(r"&(#(?:[0-9]+|[xX][0-9a-fA-F]+);?|[^\t\n\x0c <&#;]{1,32};?)")?,
                entities: serde_json::from_str(include_str!("generated/html_entities.json")).map_err(|e|e.to_string())?,
            })
        }).as_ref().map_err(|e|Failure::error(e.clone()))
    }
    fn unescape(&self, s: &str) -> String {
        self.entity
            .replace_all(s, |c: &Captures<'_>| {
                let key = &c[1];
                if let Some(number) = key.strip_prefix('#') {
                    let number = number.trim_end_matches(';');
                    let (digits, base) = number
                        .strip_prefix(['x', 'X'])
                        .map_or((number, 10), |n| (n, 16));
                    let n = u32::from_str_radix(digits, base).unwrap_or(u32::MAX);
                    if let Some(value) = self.entities.invalid_refs.get(&n.to_string()) {
                        return value.clone();
                    }
                    if self.entities.invalid_points.contains(&n) {
                        return String::new();
                    }
                    return char::from_u32(n).unwrap_or('\u{fffd}').to_string();
                }
                if let Some(value) = self.entities.entities.get(key) {
                    return value.clone();
                }
                for (i, _) in key.char_indices().rev().filter(|(i, _)| *i > 0) {
                    if let Some(value) = self.entities.entities.get(&key[..i]) {
                        return format!("{value}{}", &key[i..]);
                    }
                }
                c[0].to_owned()
            })
            .into_owned()
    }
    fn text(&self, s: &str) -> String {
        self.unescape(&self.tag.replace_all(s, ""))
            .trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
            .into()
    }
}
fn integer(c: &Captures<'_>, n: usize) -> Result<i64> {
    c.get(n)
        .ok_or_else(|| Failure::error("catalogue HTML: missing integer"))?
        .as_str()
        .parse()
        .map_err(|_| Failure::error("catalogue HTML: invalid integer"))
}
fn optional(c: &Captures<'_>, n: usize) -> Result<Document> {
    c.get(n)
        .map(|_| integer(c, n).map(Document::from))
        .transpose()
        .map(|n| n.unwrap_or(Document::Null))
}
fn item(p: &Patterns, c: &Captures<'_>) -> Result<Document> {
    Ok(Document::object([
        ("item_id", integer(c, 1)?.into()),
        ("name", p.text(&c[2]).into()),
        (
            "qty",
            if c.get(4).is_some() {
                integer(c, 4)?
            } else {
                1
            }
            .into(),
        ),
    ]))
}
pub fn build(context: &Context<'_>) -> Result<Catalogue> {
    let p = Patterns::load()?;
    let raw_dir = context.config.data_dir.join("raw");
    std::fs::create_dir_all(&raw_dir)?;
    for profession in PROFESSIONS {
        let request = Request::get(format!(
            "{}/professions/{profession}/",
            context.config.vendor_url
        ))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        );
        let response = context
            .http
            .request(&request)
            .map_err(|e| Failure::unreachable(format!("catalogue page fetch failed: {e}")))?;
        let temporary = raw_dir.join(format!("{profession}.html.tmp"));
        std::fs::write(&temporary, &response.body)?;
        if !response
            .body
            .windows(b"Recipes (".len())
            .any(|w| w == b"Recipes (")
        {
            return Err(Failure::error(crate::human::refresh::page_shape(
                profession,
            )));
        }
        std::fs::rename(temporary, raw_dir.join(format!("{profession}.html")))?;
    }
    let mut professions = vec![];
    for profession in PROFESSIONS {
        let bytes = std::fs::read(raw_dir.join(format!("{profession}.html")))?;
        let raw = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let count = p
            .count
            .captures(&raw)
            .ok_or_else(|| Failure::error("catalogue HTML: recipe count missing"))?;
        let mut recipes = vec![];
        for row in p.row.find_iter(&raw) {
            let row = row.as_str();
            let Some(recipe) = p.recipe.captures(row) else {
                continue;
            };
            let cells = p
                .cell
                .captures_iter(row)
                .map(|c| c[1].to_owned())
                .collect::<Vec<_>>();
            let Some(bands) = p.bands.captures(row) else {
                continue;
            };
            if cells.len() < 4 {
                continue;
            }
            let result = if let Some(c) = p.item.captures(&cells[2]) {
                item(p, &c)?
            } else {
                Document::object([
                    ("item_id", Document::Null),
                    ("name", p.text(&cells[2]).into()),
                    ("qty", 1i64.into()),
                ])
            };
            let reagents = p
                .item
                .captures_iter(&cells[3])
                .map(|c| item(p, &c))
                .collect::<Result<Vec<_>>>()?;
            recipes.push(Document::object([
                ("spell_id", integer(&recipe, 1)?.into()),
                ("name", p.text(&recipe[2]).into()),
                (
                    "bands",
                    Document::object([
                        ("learn", integer(&bands, 1)?.into()),
                        ("yellow", optional(&bands, 2)?),
                        ("green", optional(&bands, 3)?),
                        ("grey", optional(&bands, 4)?),
                    ]),
                ),
                ("result", result),
                ("reagents", Document::Array(reagents)),
                ("source_type", cells.get(4).map(|c| p.text(c)).into()),
            ]));
        }
        professions.push((
            profession,
            Document::object([
                ("stated_count", integer(&count, 1)?.into()),
                ("recipes", Document::Array(recipes)),
            ]),
        ));
    }
    let document = Document::object([
        ("generated_at", iso(context.clock.now()).into()),
        (
            "source",
            "wowforever.ninja profession index pages (Forever client data)".into(),
        ),
        ("professions", Document::object(professions)),
    ]);
    let catalogue: Payload<CatalogueData> = Payload::decode(document, "catalogue build")?;
    std::fs::write(
        context.config.data_dir.join("catalogue.json"),
        dumps(catalogue.document(), JsonMode::Catalogue),
    )?;
    Ok(catalogue)
}
