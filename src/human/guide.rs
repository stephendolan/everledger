use crate::{
    commands::guide::{CatalogueResponse, GuideResponse},
    human::{integer, text},
    read_roster::{GuideProgress, GuideStep},
};

fn instructions(step: &GuideStep) -> String {
    if !step.text.is_empty() {
        step.text.join(" / ")
    } else if !step.targets.is_empty() {
        format!("Target {}", step.targets.join(", "))
    } else {
        "step text unavailable".into()
    }
}

pub fn progress(guide: &GuideProgress) -> String {
    if !guide.available {
        return format!("  RestedXP unavailable ({})", text(guide.reason.as_deref()));
    }
    let details = guide.current.as_ref().map_or_else(
        || {
            format!(
                "step text unavailable ({})",
                text(guide.current_reason.as_deref())
            )
        },
        instructions,
    );
    let subgroup = guide
        .guide_subgroup
        .as_ref()
        .map_or_else(String::new, |name| format!(" / {name}"));
    let age = guide
        .file_age_seconds
        .map_or_else(String::new, |seconds| format!(", {seconds}s old"));
    format!(
        "  RestedXP | {}{subgroup} | {} step {} | {details} (SavedVariables {}{age})",
        text(guide.guide_group.as_deref()),
        text(guide.chapter.as_deref()),
        integer(guide.current_step),
        text(guide.saved_at.as_deref()),
    )
}

pub fn route(response: &GuideResponse) -> Vec<String> {
    let mut lines = vec![response.character.clone(), progress(&response.progress)];
    let (Some(true), Some(steps)) = (response.route_available, &response.steps) else {
        lines.push(format!(
            "  Guide route unavailable ({})",
            response
                .route_end
                .as_deref()
                .unwrap_or("roster did not provide steps")
        ));
        return lines;
    };
    if steps.is_empty() {
        lines.push(match &response.query {
            Some(query) => format!("  No matches for {query:?} in the installed route"),
            None => "  No more steps in the installed route".into(),
        });
    }
    for row in steps {
        let step = &row.step;
        let levels = step
            .chapter_levels
            .map_or_else(String::new, |[low, high]| format!(" | levels {low}–{high}"));
        let tags = if step.level_tags.is_empty() {
            String::new()
        } else {
            format!(" | {}", step.level_tags.join(", "))
        };
        let unit = if row.steps_from_current.abs_diff(0) == 1 {
            "step"
        } else {
            "steps"
        };
        lines.push(format!(
            "  {} step {} ({:+} installed {unit}){levels}{tags} | {}",
            step.chapter,
            step.step,
            row.steps_from_current,
            instructions(step)
        ));
        if let Some(previous) = &row.previous {
            lines.push(format!(
                "    Before this | {} step {} | {}",
                previous.chapter,
                previous.step,
                instructions(previous)
            ));
        }
    }
    if response.truncated == Some(true) {
        lines.push("  Results truncated at the requested limit".into());
    }
    if let Some(reason) = &response.route_end {
        lines.push(format!("  Installed route ends here ({reason})"));
    }
    lines
}

pub fn catalogue(response: &CatalogueResponse) -> Vec<String> {
    let mut lines = Vec::new();
    match response.groups.as_deref() {
        None => lines.push(format!(
            "Installed guides unavailable ({})",
            text(response.reason.as_deref())
        )),
        Some([]) => lines.push(match &response.group_query {
            Some(query) => format!("No installed guide groups match {query:?}"),
            None => "No installed guide groups".into(),
        }),
        Some(groups) => {
            for group in groups {
                let family = group.group.as_deref().unwrap_or("Guide family unavailable");
                lines.push(match &group.subgroup {
                    Some(subgroup) => format!("{family} / {subgroup}"),
                    None => family.to_owned(),
                });
                for chapter in &group.chapters {
                    let levels = chapter.chapter_levels.map_or_else(
                        || "levels unavailable".into(),
                        |[low, high]| format!("levels {low}–{high}"),
                    );
                    let count = match chapter.step_count {
                        Some(1) => "1 defined step".into(),
                        Some(n) => format!("{n} defined steps"),
                        None => format!(
                            "step count unavailable ({})",
                            text(chapter.reason.as_deref())
                        ),
                    };
                    lines.push(format!("  {} | {levels} | {count}", chapter.chapter));
                }
            }
        }
    }
    for source in &response.sources {
        for issue in &source.issues {
            lines.push(format!("  Coverage | {} | {issue}", source.file));
        }
    }
    lines
}
