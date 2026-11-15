use crate::{
    commands::bridge::{Color, Segment, color_at},
    human::{integer, text},
};
pub fn color(color: Color) -> &'static str {
    match color {
        Color::Orange => "orange",
        Color::Yellow => "yellow",
        Color::Green => "green",
        Color::Grey => "grey",
        Color::NotLearnable => "not learnable",
    }
}
pub fn render(prof: &str, start: i64, end: i64, segments: &[Segment<'_>]) -> Vec<String> {
    let mut lines = vec![format!("{prof} bridge {start} -> {end}")];
    for s in segments {
        lines.push(format!("{}-{}:", s.start, s.end));
        for r in &s.recipes {
            lines.push(format!(
                "  {} {} learn {} grey {} {}",
                r.name,
                color(color_at(&r.bands, s.start)),
                r.bands.learn,
                integer(r.bands.grey),
                text(r.source_type.as_deref())
            ));
        }
    }
    lines
}
