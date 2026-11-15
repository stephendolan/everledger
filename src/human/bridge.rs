use crate::{
    commands::bridge::{Color, Segment, color_at_integer},
    document::Document,
    primitives::human,
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
pub fn render(
    prof: &str,
    start: &crate::types::CliInteger,
    end: &crate::types::CliInteger,
    segments: &[Segment<'_>],
) -> Vec<String> {
    let mut lines = vec![format!("{prof} bridge {start} -> {end}")];
    for s in segments {
        lines.push(format!("{}-{}:", s.start, s.end));
        for r in &s.recipes {
            lines.push(format!(
                "  {} {} learn {} grey {} {}",
                r.name,
                color(color_at_integer(&r.bands, &s.start)),
                r.bands.learn,
                human(&Document::from(r.bands.grey)),
                human(&Document::from(r.source_type.clone()))
            ));
        }
    }
    lines
}
