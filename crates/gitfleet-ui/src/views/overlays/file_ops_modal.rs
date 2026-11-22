//! Shared modals for renaming files/folders and adding `.gitignore` rules (Glob & Regex)
//! across both Host (`Files -> All`) and GitFleet Studio Explorer.

use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::truncate_chars;
use gitfleet_git::IgnoredDirListing;
use gitfleet_security::{
    GitignorePreset, IgnoreLayer, RegexIgnorePreview, RegexRuleOrigin, build_gitignore_presets,
    build_gitignore_regex_presets, default_regex_for_entry, preview_regex_gitignore,
};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, row, text, text_input,
};
use iced::{Alignment, Element, Length};
use std::collections::{BTreeSet, HashMap};

// ── Reusable Modal State ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct RenameEntryModalState {
    pub(crate) rel_path: String,
    pub(crate) is_dir: bool,
    pub(crate) new_name: String,
    pub(crate) error: Option<String>,
}

impl RenameEntryModalState {
    pub(crate) fn new(rel_path: String, is_dir: bool) -> Self {
        let norm = rel_path.replace('\\', "/");
        let trimmed = norm.trim_matches('/').to_string();
        let current_name = trimmed
            .rsplit('/')
            .next()
            .unwrap_or(trimmed.as_str())
            .to_string();
        Self {
            rel_path: trimmed,
            is_dir,
            new_name: current_name,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AddGitignoreModalState {
    pub(crate) rel_path: String,
    pub(crate) is_dir: bool,
    pub(crate) use_regex: bool,
    pub(crate) glob_pattern: String,
    pub(crate) regex_pattern: String,
    pub(crate) regex_expand_matches: bool,
    pub(crate) layer: IgnoreLayer,
    pub(crate) allow_personal_exclude: bool,
    pub(crate) glob_presets: Vec<GitignorePreset>,
    pub(crate) regex_presets: Vec<GitignorePreset>,
    pub(crate) candidates: Vec<String>,
    pub(crate) regex_preview: Result<RegexIgnorePreview, String>,
    pub(crate) error: Option<String>,
}

impl AddGitignoreModalState {
    pub(crate) fn new(
        rel_path: String,
        is_dir: bool,
        allow_personal_exclude: bool,
        candidates: Vec<String>,
    ) -> Self {
        let norm = rel_path.replace('\\', "/");
        let trimmed = norm.trim_matches('/').to_string();
        let glob_presets = build_gitignore_presets(&trimmed, is_dir);
        let regex_presets = build_gitignore_regex_presets(&trimmed, is_dir);
        let glob_pattern = glob_presets
            .first()
            .map(|p| p.pattern.clone())
            .unwrap_or_else(|| {
                if is_dir {
                    format!("/{trimmed}/")
                } else {
                    format!("/{trimmed}")
                }
            });
        let regex_pattern = default_regex_for_entry(&trimmed, is_dir);
        let regex_preview = preview_regex_gitignore(&regex_pattern, &candidates, false);
        Self {
            rel_path: trimmed,
            is_dir,
            use_regex: false,
            glob_pattern,
            regex_pattern,
            regex_expand_matches: false,
            layer: IgnoreLayer::RootGitignore,
            allow_personal_exclude,
            glob_presets,
            regex_presets,
            candidates,
            regex_preview,
            error: None,
        }
    }

    pub(crate) fn recompute_regex_preview(&mut self) {
        self.regex_preview = preview_regex_gitignore(
            &self.regex_pattern,
            &self.candidates,
            self.regex_expand_matches,
        );
    }

    /// Returns the exact `.gitignore` rule lines ready to append, or an empty vec if invalid/empty.
    pub(crate) fn resolved_rules(&self) -> Vec<String> {
        if self.use_regex {
            match &self.regex_preview {
                Ok(preview) => preview.rules.clone(),
                Err(_) => Vec::new(),
            }
        } else {
            self.glob_pattern
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        }
    }

    pub(crate) fn target_file_label(&self) -> &'static str {
        match self.layer {
            IgnoreLayer::RootGitignore => ".gitignore",
            IgnoreLayer::PersonalExclude => ".git/info/exclude",
            IgnoreLayer::GlobalExclude => "global gitignore",
        }
    }
}

// ── Pure Helpers ─────────────────────────────────────────────────────────────

/// Validates a live rename draft against `old_rel`.
/// Returns `Ok(true)` if valid and changed, `Ok(false)` if unchanged or blank, or `Err(reason)` if invalid.
pub(crate) fn validate_rename_input(old_rel: &str, new_name: &str) -> Result<bool, String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    if trimmed == "." || trimmed == ".." || trimmed == ".git" {
        return Err(format!("'{trimmed}' is a reserved name."));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        return Err("Name cannot contain path separators ('/' or '\\').".into());
    }
    #[cfg(target_os = "windows")]
    if trimmed
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || trimmed.ends_with('.')
    {
        return Err("Name contains characters not allowed on Windows.".into());
    }
    let old_name = old_rel.rsplit('/').next().unwrap_or(old_rel);
    Ok(trimmed != old_name)
}

/// Remaps a relative path if it equals `old_rel` or (when `is_dir`) is a descendant of `old_rel`.
pub(crate) fn remap_rel_path(
    candidate: &str,
    old_rel: &str,
    new_rel: &str,
    is_dir: bool,
) -> Option<String> {
    if candidate == old_rel {
        return Some(new_rel.to_string());
    }
    if is_dir
        && let Some(rest) = candidate.strip_prefix(old_rel)
        && rest.starts_with('/')
    {
        return Some(format!("{new_rel}{rest}"));
    }
    None
}

fn insert_file_and_ancestors(set: &mut BTreeSet<String>, raw: &str) {
    let norm = raw.replace('\\', "/");
    let clean = norm.trim_matches('/');
    if clean.is_empty() {
        return;
    }
    set.insert(clean.to_string());
    let mut cursor = clean;
    while let Some((parent, _)) = cursor.rsplit_once('/') {
        if parent.is_empty() {
            break;
        }
        set.insert(format!("{parent}/"));
        cursor = parent;
    }
}

fn insert_dir_and_ancestors(set: &mut BTreeSet<String>, raw: &str) {
    let norm = raw.replace('\\', "/");
    let clean = norm.trim_matches('/');
    if clean.is_empty() {
        return;
    }
    set.insert(format!("{clean}/"));
    let mut cursor = clean;
    while let Some((parent, _)) = cursor.rsplit_once('/') {
        if parent.is_empty() {
            break;
        }
        set.insert(format!("{parent}/"));
        cursor = parent;
    }
}

/// Collects deduplicated workspace file & directory relative paths (directories end with `/`)
/// for live Regex `.gitignore` matching.
pub(crate) fn collect_workspace_ignore_candidates(
    files: &[String],
    ignored_files: &[String],
    ignored_dirs: &[String],
    ignored_dir_children: &HashMap<String, IgnoredDirListing>,
) -> Vec<String> {
    let mut set = BTreeSet::new();
    for f in files {
        insert_file_and_ancestors(&mut set, f);
    }
    for f in ignored_files {
        insert_file_and_ancestors(&mut set, f);
    }
    for d in ignored_dirs {
        insert_dir_and_ancestors(&mut set, d);
    }
    for listing in ignored_dir_children.values() {
        for f in &listing.files {
            insert_file_and_ancestors(&mut set, f);
        }
        for d in &listing.dirs {
            insert_dir_and_ancestors(&mut set, d);
        }
    }
    set.into_iter().collect()
}

// ── Shared Modal View Builders ───────────────────────────────────────────────

pub(crate) fn build_rename_entry_modal<'a>(
    p: Palette,
    modal: &'a RenameEntryModalState,
    input_id: text_input::Id,
    on_input: impl Fn(String) -> Message + 'static,
    on_submit: Message,
    on_cancel: Message,
) -> Element<'a, Message> {
    let (parent, old_name) = match modal.rel_path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", modal.rel_path.as_str()),
    };
    let validation = validate_rename_input(&modal.rel_path, &modal.new_name);
    let can_submit = matches!(validation, Ok(true));
    let validation_err = validation.err().or_else(|| modal.error.clone());

    let title = if modal.is_dir {
        "Rename Folder"
    } else {
        "Rename File"
    };
    let header_icon = if modal.is_dir {
        icons::folder(icon_size::MD, p.accent_fg)
    } else {
        icons::pencil(icon_size::MD, p.accent_fg)
    };
    let location_label = if parent.is_empty() {
        "Root".to_string()
    } else {
        format!("{}/", truncate_chars(parent, 40))
    };

    let mut input = text_input(
        if modal.is_dir {
            "New folder name…"
        } else {
            "New file name…"
        },
        &modal.new_name,
    )
    .id(input_id)
    .on_input(on_input)
    .font(theme::CODE_FONT)
    .padding(pad::INPUT)
    .size(type_scale::BODY)
    .width(Length::Fill)
    .style(theme::dark_input(p));

    if can_submit {
        input = input.on_submit(on_submit.clone());
    }

    let mut body = column![
        row![
            text("Location:")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            text(location_label)
                .size(type_scale::CAPTION)
                .font(theme::CODE_FONT)
                .color(p.text_secondary),
            theme::badge(
                truncate_chars(old_name, 30),
                p.surface_card,
                p.accent_fg,
                p.border_subtle,
                true,
            ),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center),
        input,
    ]
    .spacing(space::MD);

    if let Some(err) = validation_err {
        body = body.push(
            row![
                icons::alert_triangle(icon_size::XS, p.danger_fg),
                text(err).size(type_scale::CAPTION).color(p.danger_fg),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        );
    }

    body = body.push(
        row![
            text("Esc to cancel")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(on_cancel.clone()),
            button(text("Rename").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::primary_btn(p))
                .on_press_maybe(can_submit.then_some(on_submit)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    );

    theme::modal_shell(
        p,
        layout::MODAL_SM,
        header_icon,
        title,
        None,
        body,
        on_cancel,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_add_gitignore_modal<'a>(
    p: Palette,
    modal: &'a AddGitignoreModalState,
    input_id: text_input::Id,
    on_set_regex_mode: impl Fn(bool) -> Message + 'static,
    on_glob_input: impl Fn(String) -> Message + 'static,
    on_regex_input: impl Fn(String) -> Message + 'static,
    on_toggle_expand_matches: impl Fn(bool) -> Message + 'static,
    on_set_layer: impl Fn(IgnoreLayer) -> Message + 'static,
    on_submit: Message,
    on_cancel: Message,
) -> Element<'a, Message> {
    let resolved = modal.resolved_rules();
    let can_submit = !resolved.is_empty();
    let target_label = modal.target_file_label();

    let entry_display = if modal.is_dir {
        format!("{}/", truncate_chars(&modal.rel_path, 48))
    } else {
        truncate_chars(&modal.rel_path, 48)
    };

    let mode_switch = theme::segmented_group(
        p,
        row![
            button(
                text("Glob")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 12])
            .style(theme::segmented_btn(p, !modal.use_regex))
            .on_press(on_set_regex_mode(false)),
            button(
                text("Regex")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 12])
            .style(theme::segmented_btn(p, modal.use_regex))
            .on_press(on_set_regex_mode(true)),
        ]
        .spacing(2),
    );

    let context_row = row![
        if modal.is_dir {
            Element::from(icons::folder(icon_size::SM, p.accent_fg))
        } else {
            Element::from(icons::file_code(icon_size::SM, p.accent_fg))
        },
        text(entry_display)
            .size(type_scale::CODE)
            .font(theme::CODE_FONT)
            .color(p.text_primary),
        Space::with_width(Length::Fill),
        mode_switch,
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);

    let mut body = column![context_row].spacing(space::MD);

    if modal.allow_personal_exclude {
        let target_switch = theme::segmented_group(
            p,
            row![
                button(
                    text(".gitignore (Repo)")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([4, 10])
                .style(theme::segmented_btn(
                    p,
                    modal.layer == IgnoreLayer::RootGitignore
                ))
                .on_press(on_set_layer(IgnoreLayer::RootGitignore)),
                button(
                    text(".git/info/exclude (Local)")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([4, 10])
                .style(theme::segmented_btn(
                    p,
                    modal.layer == IgnoreLayer::PersonalExclude
                ))
                .on_press(on_set_layer(IgnoreLayer::PersonalExclude)),
            ]
            .spacing(2),
        );
        body = body.push(
            row![
                text("Target file:")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                target_switch,
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );
    }

    if !modal.use_regex {
        // ── Glob Pattern Mode ────────────────────────────────────────────────
        if !modal.glob_presets.is_empty() {
            let mut pills = row![].spacing(space::XS).align_y(Alignment::Center);
            for preset in &modal.glob_presets {
                let active = modal.glob_pattern.trim() == preset.pattern;
                pills = pills.push(
                    button(
                        text(preset.label.clone())
                            .size(type_scale::BADGE)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding(pad::PILL)
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(on_glob_input(preset.pattern.clone())),
                );
            }
            body = body.push(pills);
        }

        let mut glob_input = text_input("e.g. *.log, /build/, .env.*", &modal.glob_pattern)
            .id(input_id)
            .on_input(on_glob_input)
            .font(theme::CODE_FONT)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p));
        if can_submit {
            glob_input = glob_input.on_submit(on_submit.clone());
        }

        body = body
            .push(glob_input)
            .push(
                text("Uses standard .gitignore glob syntax (*, **, ?, trailing / for folders).")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
    } else {
        // ── Regex Mode with Live Match & Rule Preview ────────────────────────
        if !modal.regex_presets.is_empty() {
            let mut pills = row![].spacing(space::XS).align_y(Alignment::Center);
            for preset in &modal.regex_presets {
                let active = modal.regex_pattern.trim() == preset.pattern;
                pills = pills.push(
                    button(
                        text(preset.label.clone())
                            .size(type_scale::BADGE)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding(pad::PILL)
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(on_regex_input(preset.pattern.clone())),
                );
            }
            body = body.push(pills);
        }

        let mut regex_input = text_input(
            r"e.g. ^src/.*\.log$ or ^(dist|build)/?$",
            &modal.regex_pattern,
        )
        .id(input_id)
        .on_input(on_regex_input)
        .font(theme::CODE_FONT)
        .padding(pad::INPUT)
        .size(type_scale::BODY)
        .width(Length::Fill)
        .style(theme::dark_input(p));
        if can_submit {
            regex_input = regex_input.on_submit(on_submit.clone());
        }
        body = body.push(regex_input);

        let preview_box: Element<'_, Message> = match &modal.regex_preview {
            Err(err) => container(
                row![
                    icons::alert_triangle(icon_size::XS, p.danger_fg),
                    text(err.clone()).size(type_scale::CAPTION).color(p.danger_fg),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::section_panel_box(p))
            .into(),
            Ok(prev) => {
                let match_badge_fg = if prev.total_matches > 0 {
                    p.success_fg
                } else {
                    p.text_muted
                };
                let match_badge_bg = if prev.total_matches > 0 {
                    p.success_bg
                } else {
                    p.surface_card
                };
                let origin_label = match prev.origin {
                    RegexRuleOrigin::ExactGlob => "Converted to glob",
                    RegexRuleOrigin::ApproximateGlob => "Generalized glob",
                    RegexRuleOrigin::MatchedPaths => "Matched paths",
                };
                let origin_fg = match prev.origin {
                    RegexRuleOrigin::ExactGlob => p.accent_fg,
                    RegexRuleOrigin::ApproximateGlob => p.warning_fg,
                    RegexRuleOrigin::MatchedPaths => p.text_secondary,
                };

                let mut panel = column![
                    row![
                        theme::badge(
                            format!(
                                "{} match{} in workspace",
                                prev.total_matches,
                                if prev.total_matches == 1 { "" } else { "es" }
                            ),
                            match_badge_bg,
                            match_badge_fg,
                            p.border_subtle,
                            false,
                        ),
                        Space::with_width(Length::Fill),
                        theme::badge(
                            origin_label,
                            p.surface_card,
                            origin_fg,
                            p.border_subtle,
                            false,
                        ),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                ]
                .spacing(space::SM);

                if prev.can_translate_glob && prev.total_matches > 0 {
                    panel = panel.push(
                        checkbox(
                            format!(
                                "Write exact matched paths instead ({} rule{})",
                                prev.total_matches,
                                if prev.total_matches == 1 { "" } else { "s" }
                            ),
                            modal.regex_expand_matches,
                        )
                        .on_toggle(on_toggle_expand_matches)
                        .size(14)
                        .text_size(type_scale::CAPTION),
                    );
                }

                if prev.total_matches > 0 {
                    let mut samples = column![].spacing(2);
                    for path in prev.matched_paths.iter().take(4) {
                        samples = samples.push(
                            text(format!("  {path}"))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_secondary),
                        );
                    }
                    if prev.total_matches > 4 {
                        samples = samples.push(
                            text(format!("  … +{} more", prev.total_matches - 4))
                                .size(type_scale::BADGE)
                                .color(p.text_muted),
                        );
                    }
                    panel = panel.push(samples);
                }

                panel = panel.push(horizontal_rule(1).style(theme::subtle_rule(p)));

                if prev.rules.is_empty() {
                    panel = panel.push(
                        text("No workspace paths currently match this regex. Adjust the regex or switch to Glob mode to ignore future files.")
                            .size(type_scale::CAPTION)
                            .color(p.warning_fg),
                    );
                } else {
                    panel = panel.push(
                        text(format!(
                            "Rules appended to {target_label} ({}):",
                            prev.rules.len()
                        ))
                        .size(type_scale::BADGE)
                        .color(p.text_muted),
                    );
                    let mut rules_col = column![].spacing(2);
                    for rule in prev.rules.iter().take(6) {
                        rules_col = rules_col.push(
                            text(rule.clone())
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg),
                        );
                    }
                    if prev.rules.len() > 6 {
                        rules_col = rules_col.push(
                            text(format!("… +{} more rule(s)", prev.rules.len() - 6))
                                .size(type_scale::BADGE)
                                .color(p.text_muted),
                        );
                    }
                    panel = panel.push(rules_col);
                }

                container(panel)
                    .padding([10, 12])
                    .width(Length::Fill)
                    .style(theme::section_panel_box(p))
                    .into()
            }
        };

        body = body.push(preview_box);
    }

    if let Some(err) = &modal.error {
        body = body.push(
            row![
                icons::alert_triangle(icon_size::XS, p.danger_fg),
                text(err.clone()).size(type_scale::CAPTION).color(p.danger_fg),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        );
    }

    let submit_label = if resolved.len() > 1 {
        format!("Add {} Rules", resolved.len())
    } else {
        format!("Add to {target_label}")
    };

    body = body.push(
        row![
            text("Esc to cancel")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(on_cancel.clone()),
            button(text(submit_label).size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::primary_btn(p))
                .on_press_maybe(can_submit.then_some(on_submit)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center),
    );

    theme::modal_shell(
        p,
        layout::MODAL_MD,
        icons::eye_off(icon_size::MD, p.accent_fg),
        "Add to .gitignore",
        Some(theme::badge(
            target_label,
            p.surface_card,
            p.text_secondary,
            p.border_subtle,
            true,
        )),
        body,
        on_cancel,
    )
}

// ── Host Wrappers ────────────────────────────────────────────────────────────

impl GitFleetApp {
    pub(crate) fn view_rename_entry_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a RenameEntryModalState,
    ) -> Element<'a, Message> {
        build_rename_entry_modal(
            p,
            modal,
            self.rename_entry_input_id.clone(),
            Message::RenameRepoEntryInputChanged,
            Message::SubmitRenameRepoEntry,
            Message::CloseOverlays,
        )
    }

    pub(crate) fn view_add_gitignore_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a AddGitignoreModalState,
    ) -> Element<'a, Message> {
        build_add_gitignore_modal(
            p,
            modal,
            self.add_gitignore_input_id.clone(),
            Message::AddGitignoreModeChanged,
            Message::AddGitignoreGlobChanged,
            Message::AddGitignoreRegexChanged,
            Message::AddGitignoreExpandMatchesToggled,
            Message::AddGitignoreLayerChanged,
            Message::SubmitAddGitignore,
            Message::CloseOverlays,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rename_input_and_remaps_descendant_paths() {
        assert_eq!(validate_rename_input("src/app.rs", "app.rs"), Ok(false));
        assert_eq!(validate_rename_input("src/app.rs", "main.rs"), Ok(true));
        assert_eq!(validate_rename_input("src/app.rs", "App.rs"), Ok(true));
        assert!(validate_rename_input("src/app.rs", "sub/app.rs").is_err());
        assert!(validate_rename_input("src/app.rs", "..").is_err());
        assert!(validate_rename_input("src/app.rs", ".git").is_err());

        assert_eq!(
            remap_rel_path("src/old.rs", "src/old.rs", "src/new.rs", false),
            Some("src/new.rs".into())
        );
        assert_eq!(
            remap_rel_path("src/old_dir/sub/a.rs", "src/old_dir", "src/new_dir", true),
            Some("src/new_dir/sub/a.rs".into())
        );
        // Must NOT falsely match sibling prefix `src/old_dir2/a.rs`
        assert_eq!(
            remap_rel_path("src/old_dir2/a.rs", "src/old_dir", "src/new_dir", true),
            None
        );
    }

    #[test]
    fn collects_unique_workspace_files_and_directory_candidates() {
        let files = vec!["src/a.rs".into(), "src/sub/b.log".into()];
        let ignored_files = vec![".env".into()];
        let ignored_dirs = vec!["target".into()];
        let mut children = HashMap::new();
        children.insert(
            "target".into(),
            IgnoredDirListing {
                files: vec!["target/CACHEDIR.TAG".into()],
                dirs: vec!["target/debug".into()],
                truncated: false,
            },
        );
        let out =
            collect_workspace_ignore_candidates(&files, &ignored_files, &ignored_dirs, &children);
        assert!(out.contains(&".env".to_string()));
        assert!(out.contains(&"src/".to_string()));
        assert!(out.contains(&"src/sub/".to_string()));
        assert!(out.contains(&"src/sub/b.log".to_string()));
        assert!(out.contains(&"target/".to_string()));
        assert!(out.contains(&"target/debug/".to_string()));
        assert!(out.contains(&"target/CACHEDIR.TAG".to_string()));
    }
}
