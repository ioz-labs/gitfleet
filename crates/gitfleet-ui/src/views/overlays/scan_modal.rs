use crate::app::{GitFleetApp, Message, ScanLevelPreset, ScanModalState};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, tree, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use gitfleet_workspace::normalize_group_path;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, row, scrollable, text, text_input,
    tooltip,
};
use iced::{Alignment, Color, Element, Length, Padding};
use std::collections::{BTreeMap, HashSet};

const SCAN_DEPTH_CHOICES: [usize; 5] = [2, 3, 4, 5, 6];

#[derive(Default)]
struct PreviewGroupNode {
    subgroups: BTreeMap<String, PreviewGroupNode>,
    repos: Vec<usize>,
    contributing_raw_paths: Vec<String>,
}

impl PreviewGroupNode {
    fn insert_repo(
        &mut self,
        parent_prefix: Option<&str>,
        kept_segments: &[(&str, &str)],
        idx: usize,
    ) {
        let mut cursor = self;
        if let Some(prefix) = parent_prefix {
            for seg in prefix.split('/').filter(|s| !s.is_empty()) {
                cursor = cursor.subgroups.entry(seg.to_string()).or_default();
            }
        }
        for (name, raw_path) in kept_segments {
            cursor = cursor.subgroups.entry((*name).to_string()).or_default();
            if !cursor.contributing_raw_paths.iter().any(|p| p == raw_path) {
                cursor.contributing_raw_paths.push((*raw_path).to_string());
            }
        }
        cursor.repos.push(idx);
    }

    fn total_repos(&self) -> usize {
        self.repos.len()
            + self
                .subgroups
                .values()
                .map(Self::total_repos)
                .sum::<usize>()
    }

    fn collect_actionable_indices(&self, modal: &ScanModalState, out: &mut Vec<usize>) {
        for &idx in &self.repos {
            if let Some(item) = modal.repos.get(idx)
                && modal.is_repo_actionable(item)
            {
                out.push(idx);
            }
        }
        for child in self.subgroups.values() {
            child.collect_actionable_indices(modal, out);
        }
    }
}

fn owned_tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: String,
    position: tooltip::Position,
    p: Palette,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label).size(type_scale::CAPTION).color(p.text_primary))
            .padding([5, 9])
            .style(theme::tooltip_box(p)),
        position,
    )
    .gap(5)
    .into()
}

fn format_group_label(group: Option<&str>) -> &str {
    match group {
        Some(g) if !g.is_empty() => g,
        _ => "Root (no group)",
    }
}

fn render_scan_preview_node<'a>(
    node: &PreviewGroupNode,
    parent_path: &str,
    depth: usize,
    modal: &'a ScanModalState,
    existing_ws_groups: &HashSet<String>,
    p: Palette,
    out: &mut Vec<Element<'a, Message>>,
) {
    for (seg_name, child) in &node.subgroups {
        let full_path = if parent_path.is_empty() {
            seg_name.clone()
        } else {
            format!("{parent_path}/{seg_name}")
        };
        let is_new_group = !existing_ws_groups.contains(&full_path);
        let total = child.total_repos();
        let mut actionable_indices = Vec::new();
        child.collect_actionable_indices(modal, &mut actionable_indices);
        let has_actionable = !actionable_indices.is_empty();
        let has_selected_actionable = actionable_indices
            .iter()
            .any(|&idx| modal.repos.get(idx).is_some_and(|r| r.selected));
        let all_selected = has_actionable
            && actionable_indices
                .iter()
                .all(|&idx| modal.repos.get(idx).is_some_and(|r| r.selected));
        let left_pad = 6.0 + (depth as f32) * tree::INDENT_STEP;

        let check_el: Element<'a, Message> = if has_actionable {
            let toggle_indices = actionable_indices.clone();
            checkbox("", all_selected)
                .size(tree::CHECKBOX_SIZE)
                .spacing(0)
                .style(theme::tree_checkbox(p))
                .on_toggle(move |v| Message::ScanModalToggleRepos(toggle_indices.clone(), v))
                .into()
        } else {
            Space::with_width(Length::Fixed(tree::CHECKBOX_SIZE)).into()
        };

        let mut grp_row = row![
            check_el,
            icons::folder_open(tree::LEAD_ICON_SIZE, p.accent_fg),
            theme::clipped_line(
                row![
                    text(seg_name.clone()).size(type_scale::BODY).color(
                        if has_selected_actionable {
                            p.text_primary
                        } else {
                            p.text_muted
                        }
                    ),
                    theme::badge(
                        format!("{total}"),
                        p.surface_card,
                        p.text_muted,
                        p.border_subtle,
                        false,
                    ),
                    if is_new_group && has_selected_actionable {
                        Element::from(theme::badge(
                            "new",
                            p.success_bg,
                            p.success_fg,
                            p.border_subtle,
                            false,
                        ))
                    } else {
                        Space::with_width(Length::Shrink).into()
                    },
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                Length::Fill,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if !child.contributing_raw_paths.is_empty() {
            let raw_to_drop = child.contributing_raw_paths.clone();
            let drop_btn = button(
                row![
                    icons::minus(11.0, p.text_secondary),
                    text("Drop")
                        .size(type_scale::BADGE)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(3)
                .align_y(Alignment::Center),
            )
            .padding([2, 6])
            .style(theme::subtle_btn(p))
            .on_press(Message::ScanModalDropGroupNodes(raw_to_drop));

            grp_row = grp_row.push(owned_tip(
                drop_btn,
                format!(
                    "Drop group '{seg_name}' — repositories stay selected and move up one level"
                ),
                tooltip::Position::Left,
                p,
            ));
        }

        out.push(
            container(grp_row)
                .padding(Padding {
                    top: 3.0,
                    right: 8.0,
                    bottom: 3.0,
                    left: left_pad,
                })
                .width(Length::Fill)
                .into(),
        );

        render_scan_preview_node(
            child,
            &full_path,
            depth + 1,
            modal,
            existing_ws_groups,
            p,
            out,
        );
    }

    for &repo_idx in &node.repos {
        let Some(item) = modal.repos.get(repo_idx) else {
            continue;
        };
        let actionable = modal.is_repo_actionable(item);
        let target_group = modal.resolved_group_for(item);
        let exists_diff =
            item.candidate.already_in_workspace && item.candidate.existing_group != target_group;
        let left_pad = 6.0 + (depth as f32) * tree::INDENT_STEP;

        let lead_el: Element<'a, Message> = if actionable {
            checkbox("", item.selected)
                .size(tree::CHECKBOX_SIZE)
                .spacing(0)
                .style(theme::tree_checkbox(p))
                .on_toggle(move |v| Message::ScanModalToggleRepo(repo_idx, v))
                .into()
        } else {
            container(icons::check(11.0, p.text_muted))
                .width(Length::Fixed(tree::CHECKBOX_SIZE))
                .center_x(Length::Fixed(tree::CHECKBOX_SIZE))
                .into()
        };

        let dot_color = if !item.candidate.already_in_workspace {
            p.accent_fg
        } else if exists_diff && modal.update_existing_groups {
            p.warning_fg
        } else {
            p.text_muted
        };

        let status_el: Element<'a, Message> = if !item.candidate.already_in_workspace {
            Space::with_width(Length::Shrink).into()
        } else if !exists_diff {
            theme::badge(
                "Added",
                p.surface_card,
                p.text_muted,
                p.border_subtle,
                false,
            )
        } else if modal.update_existing_groups {
            let old_g = format_group_label(item.candidate.existing_group.as_deref());
            let new_g = format_group_label(target_group.as_deref());
            owned_tip(
                theme::badge(
                    "Move",
                    p.fill_selected,
                    p.accent_fg,
                    p.accent_primary,
                    false,
                ),
                format!("Move group: {old_g} → {new_g}"),
                tooltip::Position::Left,
                p,
            )
        } else {
            let old_g = format_group_label(item.candidate.existing_group.as_deref());
            let new_g = format_group_label(target_group.as_deref());
            owned_tip(
                theme::badge(
                    "Exists",
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false,
                ),
                format!(
                    "Already in workspace ({old_g}). Enable 'Regroup existing' below to move to {new_g}."
                ),
                tooltip::Position::Left,
                p,
            )
        };

        let repo_row = row![
            lead_el,
            container(Space::new(7, 7)).style(theme::dot_box(dot_color)),
            theme::clipped_line(
                row![
                    text(item.candidate.name.clone())
                        .size(type_scale::BODY)
                        .color(if actionable {
                            p.text_primary
                        } else {
                            p.text_muted
                        }),
                    text(item.candidate.relative_path.clone())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                Length::Fill,
            ),
            status_el,
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        out.push(
            container(repo_row)
                .padding(Padding {
                    top: 3.0,
                    right: 8.0,
                    bottom: 3.0,
                    left: left_pad,
                })
                .width(Length::Fill)
                .into(),
        );
    }
}

impl GitFleetApp {
    pub(crate) fn view_scan_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a ScanModalState,
    ) -> Element<'a, Message> {
        let ws = self.service.workspace.current();
        let existing_ws_groups: HashSet<String> = ws
            .groups
            .iter()
            .map(|g| normalize_group_path(g))
            .filter(|g| !g.is_empty())
            .collect();
        let counts = modal.counts(&ws.groups);

        // 1. Folder & Scan Depth strip
        let mut depth_pills = row![text("Depth:").size(type_scale::CAPTION).color(p.text_muted),]
            .spacing(4)
            .align_y(Alignment::Center);
        for d in SCAN_DEPTH_CHOICES {
            depth_pills = depth_pills.push(
                button(
                    text(format!("{d}"))
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([2, 7])
                .style(theme::filter_pill_btn(p, modal.max_depth == d))
                .on_press(Message::ScanModalSetDepth(d)),
            );
        }

        let folder_strip = row![
            icons::folder_open(icon_size::SM, p.accent_fg),
            theme::clipped_line(
                text(modal.root.display().to_string())
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_primary),
                Length::Fill,
            ),
            button(text("Change…").size(type_scale::CAPTION))
                .padding([3, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::PickScanFolder(modal.normalized_parent_group())),
            depth_pills,
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        // 2. Group Levels control card
        let mut presets_row = row![].spacing(4).align_y(Alignment::Center);
        if !modal.levels.is_empty() {
            let preset_btn = |label: &'a str, preset: ScanLevelPreset, hint: &'a str| {
                tip(
                    button(
                        text(label)
                            .size(type_scale::BADGE)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, modal.is_preset_active(preset)))
                    .on_press(Message::ScanModalApplyPreset(preset)),
                    hint,
                    tooltip::Position::Top,
                    p,
                )
            };

            presets_row = presets_row.push(preset_btn(
                "All Levels",
                ScanLevelPreset::All,
                "Include every discovered folder level as nested groups",
            ));
            if modal.levels.len() >= 2 {
                presets_row = presets_row.push(preset_btn(
                    "1 Level",
                    ScanLevelPreset::MaxOne,
                    "Keep only a single clean group level",
                ));
            }
            if modal.levels.len() >= 3 {
                presets_row = presets_row.push(preset_btn(
                    "2 Levels",
                    ScanLevelPreset::MaxTwo,
                    "Keep at most 2 nested group levels",
                ));
            }
            presets_row = presets_row.push(preset_btn(
                "Flat",
                ScanLevelPreset::Flat,
                "Drop all folder levels (import without auto-groups)",
            ));
        }

        let mut levels_card_col = column![
            row![
                icons::folder_tree(icon_size::SM, p.accent_fg),
                text("Group Levels")
                    .size(type_scale::SUBHEADING)
                    .color(p.text_primary),
                Space::with_width(Length::Fill),
                presets_row,
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        ]
        .spacing(space::XS);

        if modal.levels.is_empty() {
            levels_card_col = levels_card_col.push(
                text("No subfolder group levels detected — repositories will import to workspace root or the optional parent group below.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        } else {
            let mut chips_row = row![].spacing(5).align_y(Alignment::Center);
            for (idx, lvl) in modal.levels.iter().enumerate() {
                if idx > 0 {
                    chips_row = chips_row.push(icons::chevron_right(11.0, p.text_muted));
                }
                let enabled = modal.enabled_levels.get(idx).copied().unwrap_or(true);
                let chip_label = if lvl.is_root_folder {
                    let root_name = lvl
                        .folder_names
                        .first()
                        .map(String::as_str)
                        .unwrap_or("root");
                    format!("Root: {}", truncate_chars(root_name, 18))
                } else {
                    let display_num = if modal.has_root_level { idx } else { idx + 1 };
                    let preview_names: Vec<&str> = lvl
                        .folder_names
                        .iter()
                        .take(2)
                        .map(String::as_str)
                        .collect();
                    let extra = lvl.folder_names.len().saturating_sub(2);
                    let sample = if extra > 0 {
                        format!("{} (+{extra})", preview_names.join(", "))
                    } else {
                        preview_names.join(", ")
                    };
                    format!("L{display_num}: {}", truncate_chars(&sample, 24))
                };
                let all_names = lvl.folder_names.join(", ");
                let tip_text = if enabled {
                    format!("Click to drop this group level\nFolders: {all_names}")
                } else {
                    format!("Dropped — click to restore this group level\nFolders: {all_names}")
                };

                let chip_btn = button(
                    row![
                        if enabled {
                            icons::check(11.0, p.accent_fg)
                        } else {
                            icons::close_x(10.0, p.text_muted)
                        },
                        text(chip_label)
                            .size(type_scale::CAPTION)
                            .color(if enabled { p.accent_fg } else { p.text_muted })
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::filter_pill_btn(p, enabled))
                .on_press(Message::ScanModalToggleLevel(idx));

                chips_row =
                    chips_row.push(owned_tip(chip_btn, tip_text, tooltip::Position::Top, p));
            }

            if !modal.skipped_raw_nodes.is_empty() {
                let n = modal.skipped_raw_nodes.len();
                chips_row = chips_row.push(Space::with_width(Length::Fixed(6.0))).push(
                    button(
                        row![
                            icons::undo(11.0, p.warning_fg),
                            text(format!("Restore {n} dropped"))
                                .size(type_scale::BADGE)
                                .color(p.warning_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ScanModalRestoreDroppedGroups),
                );
            }

            levels_card_col = levels_card_col.push(
                scrollable(container(chips_row).padding(Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: 6.0,
                    left: 0.0,
                }))
                .width(Length::Fill)
                .direction(theme::slim_horizontal_direction())
                .style(theme::slim_scrollable(p)),
            );
        }

        let mut parent_row = row![
            text("Parent group (optional):")
                .size(type_scale::CAPTION)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
            text_input(
                "None (workspace root) — or e.g. Work/ClientA",
                &modal.target_parent_group
            )
            .on_input(Message::ScanModalParentGroupChanged)
            .padding(pad::INPUT_COMPACT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        if !modal.target_parent_group.trim().is_empty() {
            parent_row = parent_row.push(tip(
                button(icons::close_x(11.0, p.text_secondary))
                    .padding([5, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ScanModalParentGroupChanged(String::new())),
                "Clear parent group",
                tooltip::Position::Top,
                p,
            ));
        }

        levels_card_col = levels_card_col.push(parent_row);

        let levels_card = container(levels_card_col)
            .padding(pad::CARD)
            .width(Length::Fill)
            .style(theme::section_panel_box(p));

        // 3. Build Live Preview Tree
        let parent_prefix = modal.normalized_parent_group();
        let mut preview_root = PreviewGroupNode::default();
        for (idx, item) in modal.repos.iter().enumerate() {
            let kept: Vec<(&str, &str)> = item
                .candidate
                .raw_segments
                .iter()
                .enumerate()
                .filter(|(level_idx, seg)| {
                    modal
                        .enabled_levels
                        .get(*level_idx)
                        .copied()
                        .unwrap_or(true)
                        && !modal.skipped_raw_nodes.contains(&seg.raw_path)
                })
                .map(|(_, seg)| (seg.name.as_str(), seg.raw_path.as_str()))
                .collect();
            preview_root.insert_repo(parent_prefix.as_deref(), &kept, idx);
        }

        let all_actionable_selected =
            counts.actionable_total > 0 && counts.actionable_selected == counts.actionable_total;

        let mut stats_parts = Vec::new();
        if counts.selected_new > 0 {
            stats_parts.push(format!("{} new", counts.selected_new));
        }
        if counts.selected_regroup > 0 {
            stats_parts.push(format!("{} move", counts.selected_regroup));
        }
        if counts.existing_same > 0 {
            stats_parts.push(format!("{} added", counts.existing_same));
        }
        let stats_label = if stats_parts.is_empty() {
            format!("{} discovered", counts.total_repos)
        } else {
            stats_parts.join(" · ")
        };

        let master_check: Element<'a, Message> = if counts.actionable_total > 0 {
            checkbox("", all_actionable_selected)
                .size(tree::CHECKBOX_SIZE)
                .spacing(0)
                .style(theme::tree_checkbox(p))
                .on_toggle(Message::ScanModalToggleAll)
                .into()
        } else {
            Space::with_width(Length::Fixed(tree::CHECKBOX_SIZE)).into()
        };

        let preview_header = container(
            row![
                master_check,
                text("Preview Structure (Groups & Repositories)")
                    .size(type_scale::BADGE)
                    .color(p.text_muted),
                Space::with_width(Length::Fill),
                text(stats_label)
                    .size(type_scale::BADGE)
                    .color(p.text_secondary),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(theme::table_col_header_box(p));

        let mut tree_rows: Vec<Element<'a, Message>> = Vec::new();
        if modal.scanning {
            tree_rows.push(
                container(
                    text("Scanning folder for Git repositories…")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([16, 12])
                .into(),
            );
        } else if modal.repos.is_empty() {
            tree_rows.push(
                container(
                    text(format!(
                        "No Git repositories found at depth {}. Increase depth above or choose another folder.",
                        modal.max_depth
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                )
                .padding([16, 12])
                .into(),
            );
        } else {
            render_scan_preview_node(
                &preview_root,
                "",
                0,
                modal,
                &existing_ws_groups,
                p,
                &mut tree_rows,
            );
        }

        let preview_box = container(
            column![
                preview_header,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                slim_scroll(column(tree_rows).spacing(1).width(Length::Fill), p)
                    .height(Length::Fixed(230.0)),
            ]
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        // 4. Footer
        let left_footer: Element<'a, Message> = if counts.existing_movable > 0 {
            checkbox(
                format!("Regroup existing repos ({})", counts.existing_movable),
                modal.update_existing_groups,
            )
            .size(14)
            .text_size(type_scale::CAPTION)
            .on_toggle(Message::ScanModalToggleUpdateExisting)
            .into()
        } else if counts.new_groups_count > 0 {
            text(format!(
                "Will auto-create {} new group(s)",
                counts.new_groups_count
            ))
            .size(type_scale::CAPTION)
            .color(p.success_fg)
            .into()
        } else if counts.active_groups_count > 0 {
            text(format!(
                "Using {} existing workspace group(s)",
                counts.active_groups_count
            ))
            .size(type_scale::CAPTION)
            .color(p.text_muted)
            .into()
        } else {
            text("No groups selected — repositories will be added at workspace root")
                .size(type_scale::CAPTION)
                .color(p.text_muted)
                .into()
        };

        let confirm_label = match (counts.selected_new > 0, counts.selected_regroup > 0) {
            (true, true) => format!(
                "Import {} & Regroup {}",
                counts.selected_new, counts.selected_regroup
            ),
            (true, false) => format!(
                "Import {} Repo{}",
                counts.selected_new,
                if counts.selected_new == 1 { "" } else { "s" }
            ),
            (false, true) => format!("Apply Groups ({})", counts.selected_regroup),
            (false, false) => "Import".to_string(),
        };
        let can_confirm = !modal.scanning && counts.actionable_selected > 0;

        let footer = row![
            left_footer,
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseScanModal),
            button(
                row![
                    icons::folder_plus(icon_size::XS, Color::WHITE),
                    text(confirm_label).size(type_scale::BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_confirm.then_some(Message::ConfirmScanImport)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let body = column![
            folder_strip,
            levels_card,
            preview_box,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::SM);

        let badge_label = if modal.scanning {
            "Scanning…".to_string()
        } else {
            format!(
                "{} repo(s) · {} group(s)",
                counts.total_repos, counts.active_groups_count
            )
        };

        theme::modal_shell(
            p,
            layout::MODAL_LG,
            Element::from(icons::folder_search(icon_size::MD, p.accent_fg)),
            "Scan Folder & Auto-Group",
            Some(theme::badge(
                badge_label,
                p.surface_card,
                p.accent_fg,
                p.border_subtle,
                false,
            )),
            body,
            Message::CloseScanModal,
        )
    }
}
