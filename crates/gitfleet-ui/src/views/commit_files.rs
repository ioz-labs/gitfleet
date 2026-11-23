use crate::app::{
    ContextMenuTarget, FilesViewMode, GitFleetApp, GraphSelection, Message, PendingConfirm,
    ResizeTarget,
};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{parent_dir, slim_scroll_h, tip};
use gitfleet_git::{ConflictStrategy, FileChange, FileChangeKind, ResetMode};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, mouse_area, row, text,
    text_editor, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Font, Length, font, keyboard, mouse};
use std::collections::{BTreeMap, HashMap, HashSet};

const MEDIUM_FONT: Font = Font {
    weight: font::Weight::Medium,
    ..Font::DEFAULT
};

fn file_state_colors(kind: FileChangeKind, p: Palette) -> (Color, Color) {
    match kind {
        FileChangeKind::Added => (p.success_fg, p.success_bg),
        FileChangeKind::Modified => (p.warning_fg, p.warning_bg),
        FileChangeKind::Deleted | FileChangeKind::Conflicted => (p.danger_fg, p.danger_bg),
        FileChangeKind::Renamed | FileChangeKind::Copied => {
            (p.violet_fg, Color { a: 0.16, ..p.violet_fg })
        }
        FileChangeKind::Untracked => (p.accent_fg, Color { a: 0.15, ..p.accent_primary }),
    }
}

fn split_portions(ratio: f32) -> (u16, u16) {
    let first = (ratio.clamp(0.22, 0.78) * 1000.0).round() as u16;
    let second = 1000u16.saturating_sub(first).max(150);
    (first.max(150), second)
}

/// Applies desktop-standard file selection (`Click`, `Shift+Click` range, `Ctrl+Click` toggle)
/// within the visible ordered file list of a section (Unstaged, Staged, or Commit).
pub(crate) fn apply_file_selection(
    selected: &mut HashSet<(String, bool)>,
    anchor: &mut Option<FileChange>,
    visible: &[FileChange],
    clicked: &FileChange,
    shift: bool,
    command: bool,
) {
    let key = (clicked.path.clone(), clicked.staged);
    if shift {
        let anchor_idx = anchor.as_ref().and_then(|a| {
            visible
                .iter()
                .position(|f| f.path == a.path && f.staged == a.staged)
        });
        let target_idx = visible
            .iter()
            .position(|f| f.path == clicked.path && f.staged == clicked.staged);
        if let (Some(a_idx), Some(t_idx)) = (anchor_idx, target_idx) {
            if !command {
                selected.clear();
            } else {
                selected.retain(|(_, staged)| *staged == clicked.staged);
            }
            let (lo, hi) = if a_idx <= t_idx {
                (a_idx, t_idx)
            } else {
                (t_idx, a_idx)
            };
            for f in &visible[lo..=hi] {
                selected.insert((f.path.clone(), f.staged));
            }
            return;
        }
    }

    if command {
        // Keep selection within one section so Stage / Unstage stay unambiguous
        selected.retain(|(_, staged)| *staged == clicked.staged);
        if !selected.remove(&key) {
            selected.insert(key);
        }
        *anchor = Some(clicked.clone());
    } else {
        selected.clear();
        selected.insert(key);
        *anchor = Some(clicked.clone());
    }
}

#[derive(Default)]
struct FolderTreeNode {
    children: BTreeMap<String, FolderTreeNode>,
    total_files: usize,
    has_dirty: bool,
    has_non_ignored: bool,
    has_ignored: bool,
    lazy_ignored_dir: bool,
    lazy_loaded: bool,
}

impl FolderTreeNode {
    fn is_ignored_only(&self) -> bool {
        self.has_ignored && !self.has_non_ignored
    }

    fn insert_file(&mut self, file_path: &str, dirty: bool, ignored: bool) {
        self.total_files += 1;
        if dirty {
            self.has_dirty = true;
        }
        if ignored {
            self.has_ignored = true;
        } else {
            self.has_non_ignored = true;
        }
        let dir = parent_dir(file_path);
        if dir.is_empty() {
            return;
        }
        let mut cursor = self;
        for seg in dir.split('/').filter(|s| !s.is_empty()) {
            cursor = cursor.children.entry(seg.to_string()).or_default();
            cursor.total_files += 1;
            if dirty {
                cursor.has_dirty = true;
            }
            if ignored {
                cursor.has_ignored = true;
            } else {
                cursor.has_non_ignored = true;
            }
        }
    }

    fn insert_ignored_dir(
        &mut self,
        dir_path: &str,
        cache: &HashMap<String, gitfleet_git::IgnoredDirListing>,
    ) {
        let trimmed = dir_path.trim_matches('/');
        if trimmed.is_empty() {
            return;
        }
        self.has_ignored = true;
        let mut cursor = self;
        for seg in trimmed.split('/').filter(|s| !s.is_empty()) {
            cursor = cursor.children.entry(seg.to_string()).or_default();
            cursor.has_ignored = true;
        }
        cursor.lazy_ignored_dir = true;
        Self::populate_lazy_dir(cursor, trimmed, cache);
    }

    fn populate_lazy_dir(
        node: &mut FolderTreeNode,
        full_path: &str,
        cache: &HashMap<String, gitfleet_git::IgnoredDirListing>,
    ) {
        let Some(listing) = cache.get(full_path) else {
            return;
        };
        node.lazy_loaded = true;
        node.total_files += listing.files.len();
        for child_dir in &listing.dirs {
            let child_name = child_dir.rsplit('/').next().unwrap_or(child_dir);
            let child = node.children.entry(child_name.to_string()).or_default();
            child.has_ignored = true;
            child.lazy_ignored_dir = true;
            Self::populate_lazy_dir(child, child_dir, cache);
        }
    }
}

fn is_folder_expanded(full_path: &str, expanded_set: &HashSet<String>) -> bool {
    if full_path.is_empty() {
        !expanded_set.contains("")
    } else {
        expanded_set.contains(full_path)
    }
}

struct FolderTreeCtx<'a> {
    selected_dir: &'a str,
    expanded_dirs: &'a HashSet<String>,
    p: Palette,
}

fn render_folder_tree<'a>(
    node: &FolderTreeNode,
    label: &str,
    full_path: &str,
    depth: usize,
    ctx: &FolderTreeCtx<'_>,
    out: &mut Vec<Element<'a, Message>>,
    max_tree_w: &mut f32,
) {
    let p = ctx.p;
    let dimmed = depth > 0 && node.is_ignored_only();
    let has_children = !node.children.is_empty() || (node.lazy_ignored_dir && !node.lazy_loaded);
    let expanded = has_children && is_folder_expanded(full_path, ctx.expanded_dirs);
    let is_sel = ctx.selected_dir == full_path;
    let indent = (depth as f32) * 12.0;
    let hide_count = node.lazy_ignored_dir
        && (!node.lazy_loaded || (node.total_files == 0 && !node.children.is_empty()));
    let count_str = if hide_count {
        String::new()
    } else {
        format!("({})", node.total_files)
    };
    let count_w = if count_str.is_empty() {
        0.0
    } else {
        5.0 + theme::estimate_text_width(&count_str, type_scale::BADGE, false)
    };
    let est_w = indent
        + 15.0 // chevron slot
        + 2.0
        + 12.0 // button h-pad
        + 12.0 // folder icon
        + 5.0
        + theme::estimate_text_width(label, type_scale::CAPTION, false)
        + count_w
        + 6.0;
    if est_w > *max_tree_w {
        *max_tree_w = est_w;
    }

    let chevron: Element<'a, Message> = if has_children {
        button(if expanded {
            icons::chevron_down(
                11.0,
                if dimmed {
                    Color { a: 0.55, ..p.text_muted }
                } else {
                    p.text_secondary
                },
            )
        } else {
            icons::chevron_right(
                11.0,
                if dimmed {
                    Color { a: 0.55, ..p.text_muted }
                } else {
                    p.text_muted
                },
            )
        })
        .padding([2, 2])
        .width(Length::Fixed(15.0))
        .style(theme::vscode_icon_btn(p, false))
        .on_press(Message::ToggleAllFilesDirNode(full_path.to_string()))
        .into()
    } else {
        Space::with_width(Length::Fixed(15.0)).into()
    };

    let folder_color = if dimmed {
        Color { a: 0.56, ..p.text_muted }
    } else if node.has_dirty {
        p.warning_fg
    } else {
        p.accent_fg
    };
    let folder_icon: Element<'a, Message> = if expanded {
        icons::folder_open(12.0, folder_color).into()
    } else {
        icons::folder(12.0, folder_color).into()
    };

    let label_color = if dimmed {
        if is_sel {
            p.text_secondary
        } else {
            Color { a: 0.72, ..p.text_muted }
        }
    } else if is_sel {
        p.text_primary
    } else if node.has_dirty {
        p.warning_fg
    } else {
        p.text_primary
    };

    let mut label_row = row![
        text(label.to_string())
            .size(type_scale::CAPTION)
            .color(label_color),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    if !count_str.is_empty() {
        label_row = label_row.push(
            text(count_str)
                .size(type_scale::BADGE)
                .color(if dimmed {
                    Color { a: 0.58, ..p.text_muted }
                } else {
                    p.text_muted
                }),
        );
    }

    let dir_btn = button(
        row![folder_icon, theme::clipped_line(label_row, Length::Fill)]
            .spacing(5)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([3, 6])
    .style(theme::repo_row_btn(p, is_sel))
    .on_press(Message::SelectAllFilesDir(full_path.to_string()));

    let row_el: Element<'a, Message> = row![
        Space::with_width(Length::Fixed(indent)),
        chevron,
        dir_btn,
    ]
    .spacing(2)
    .align_y(Alignment::Center)
    .into();

    if depth > 0 {
        out.push(
            mouse_area(row_el)
                .on_right_press(Message::OpenContextMenu(ContextMenuTarget::RepoDir {
                    path: full_path.to_string(),
                }))
                .into(),
        );
    } else {
        out.push(row_el);
    }

    if expanded {
        for (child_name, child_node) in &node.children {
            let child_path = if full_path.is_empty() {
                child_name.clone()
            } else {
                format!("{full_path}/{child_name}")
            };
            render_folder_tree(
                child_node,
                child_name,
                &child_path,
                depth + 1,
                ctx,
                out,
                max_tree_w,
            );
        }
    }
}

impl GitFleetApp {
    /// Resizable Top-Right-Top zone: Commit Composer (Working Tree) or Commit Details Card.
    pub(crate) fn view_commit_pane(&self, p: Palette) -> Element<'_, Message> {
        let active_status = self.active_status();

        match &self.graph_selection {
            GraphSelection::WorkingTree => {
                let staged_n = active_status.map(|s| s.staged_count).unwrap_or(0);
                let summary_len = self.commit_summary.chars().count();
                let body_lines = if self.commit_editor.text().trim().is_empty() {
                    0
                } else {
                    self.commit_editor.line_count()
                };

                let mut wide_hdr_left = row![
                    text("Commit")
                        .size(type_scale::CAPTION)
                        .font(theme::UI_FONT_MEDIUM)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center);
                if staged_n > 0 {
                    wide_hdr_left = wide_hdr_left.push(theme::badge(
                        format!("{staged_n} staged"),
                        p.success_bg,
                        p.success_fg,
                        p.border_subtle,
                        false,
                    ));
                }

                let counter_color = if summary_len > 72 {
                    p.danger_fg
                } else if summary_len > 50 {
                    p.warning_fg
                } else if summary_len > 0 {
                    p.success_fg
                } else {
                    p.text_muted
                };
                let counter_label = if body_lines > 0 {
                    format!("{summary_len}/50 · +{body_lines}L")
                } else {
                    format!("{summary_len}/50")
                };

                let wide_hdr = row![
                    theme::clipped_line(wide_hdr_left, Length::Fill),
                    theme::clipped_line(
                        text(counter_label)
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(counter_color)
                            .wrapping(text::Wrapping::None),
                        Length::Shrink,
                    ),
                ]
                .spacing(space::XS)
                .width(Length::Fill)
                .align_y(Alignment::Center);

                let mut compact_hdr_left = row![
                    text("Commit")
                        .size(type_scale::BODY)
                        .color(p.text_primary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(space::XXS)
                .align_y(Alignment::Center);
                if staged_n > 0 {
                    compact_hdr_left = compact_hdr_left.push(theme::badge(
                        format!("{staged_n}"),
                        p.success_bg,
                        p.success_fg,
                        p.border_subtle,
                        false,
                    ));
                }

                // In narrow panes, park the Amend toggle cleanly on the empty right side of the
                // Commit header so the commit message editor retains generous vertical height.
                let compact_hdr = row![
                    theme::clipped_line(compact_hdr_left, Length::Fill),
                    tip(
                        theme::clipped_line(
                            checkbox("Amend", self.amend_commit)
                                .on_toggle(Message::AmendToggled)
                                .size(13)
                                .spacing(5)
                                .text_size(type_scale::CAPTION)
                                .style(theme::tree_checkbox(p)),
                            Length::Shrink,
                        ),
                        "Amend last commit",
                        tooltip::Position::Bottom,
                        p,
                    ),
                ]
                .spacing(space::XS)
                .width(Length::Fill)
                .align_y(Alignment::Center);

                let hdr = theme::adaptive_width(235.0, Length::Fill, wide_hdr, compact_hdr);

                let summary_input = text_input("Summary (50 chars recommended)…", &self.commit_summary)
                    .id(self.commit_summary_id.clone())
                    .on_input(Message::CommitSummaryChanged)
                    .on_submit(Message::CommitSubmitted)
                    .padding([5, 8])
                    .size(type_scale::BODY)
                    .style(theme::dark_input(p));

                let mut cc_chips = row![].spacing(3).align_y(Alignment::Center);
                for (chip_lbl, prefix) in [
                    ("feat", "feat: "),
                    ("fix", "fix: "),
                    ("refactor", "refactor: "),
                    ("perf", "perf: "),
                    ("docs", "docs: "),
                    ("chore", "chore: "),
                ] {
                    let chip_active = self.commit_summary.trim_start().starts_with(prefix.trim());
                    cc_chips = cc_chips.push(
                        button(
                            text(chip_lbl)
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(if chip_active {
                                    p.accent_fg
                                } else {
                                    p.text_muted
                                }),
                        )
                        .padding([1, 5])
                        .style(theme::toolbar_btn(p, chip_active))
                        .on_press(Message::ApplyConventionalPrefix(prefix)),
                    );
                }
                let cc_strip = slim_scroll_h(cc_chips, 230.0, p);

                let commit_box = text_editor(&self.commit_editor)
                    .placeholder("Description body (optional · Ctrl+Enter to commit)")
                    .on_action(Message::CommitMessageAction)
                    .key_binding(|key_press| {
                        if key_press.status == text_editor::Status::Focused
                            && key_press.modifiers.command()
                            && matches!(
                                key_press.key.as_ref(),
                                keyboard::Key::Named(keyboard::key::Named::Enter)
                            )
                        {
                            return Some(text_editor::Binding::Custom(
                                if key_press.modifiers.shift() {
                                    Message::CommitAndPushSubmitted
                                } else {
                                    Message::CommitSubmitted
                                },
                            ));
                        }
                        text_editor::Binding::from_key_press(key_press)
                    })
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .height(Length::Fill)
                    .style(theme::dark_editor(p));

                let mut alerts_col = column![].spacing(space::XS);
                let mut has_alerts = false;

                if let Some(t) = &self.amend_target
                    && t.pushed_to_upstream
                {
                    has_alerts = true;
                    alerts_col = alerts_col.push(theme::clipped_line(
                        text(format!(
                            "Warning: HEAD ({}) is already pushed to upstream.",
                            t.short_oid
                        ))
                        .size(type_scale::BADGE)
                        .color(p.danger_fg),
                        Length::Fill,
                    ));
                }

                for finding in self.staged_secrets.iter().take(2) {
                    has_alerts = true;
                    alerts_col = alerts_col.push(
                        container(
                            row![
                                theme::clipped_line(
                                    text(format!(
                                        "Secret ({}) in {}:{}",
                                        finding.rule, finding.file_path, finding.line
                                    ))
                                    .size(type_scale::BADGE)
                                    .color(p.danger_fg),
                                    Length::Fill,
                                ),
                                tip(
                                    button(icons::check(icon_size::XS, p.text_secondary))
                                        .padding([3, 5])
                                        .style(theme::subtle_btn(p))
                                        .on_press(Message::AllowSecretLine(finding.id.clone())),
                                    "Allow this secret finding",
                                    tooltip::Position::Left,
                                    p,
                                ),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                    );
                }

                for big in self.oversized_files.iter().take(2) {
                    has_alerts = true;
                    let mb = big.bytes as f64 / (1024.0 * 1024.0);
                    alerts_col = alerts_col.push(
                        container(
                            row![
                                theme::clipped_line(
                                    text(format!("Oversized: {} ({:.1} MB)", big.file_path, mb))
                                        .size(type_scale::BADGE)
                                        .color(p.warning_fg),
                                    Length::Fill,
                                ),
                                tip(
                                    button(icons::close_x(icon_size::XS, p.text_secondary))
                                        .padding([3, 5])
                                        .style(theme::subtle_btn(p))
                                        .on_press(Message::IgnoreOversizedFile(
                                            big.file_path.clone()
                                        )),
                                    "Add oversized file to .gitignore",
                                    tooltip::Position::Left,
                                    p,
                                ),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                }

                let encrypt_needed = self.vault_summary.encrypt_needed_count();
                let restore_needed = self.vault_summary.restore_needed_count();
                let profile_tag = if self.vault_summary.is_default_profile() {
                    String::new()
                } else {
                    format!(" [{}]", self.vault_summary.active_profile)
                };
                let vault_fname = self.vault_summary.vault_file_name.as_str();
                if self.vault_summary.vault_locked {
                    has_alerts = true;
                    let repo_name = self
                        .current_repo()
                        .map(|r| r.name)
                        .unwrap_or_else(|| "repository".into());
                    let key_hint = self
                        .vault_summary
                        .manifest_key_name
                        .as_deref()
                        .map(|k| format!(" (key: {k})"))
                        .unwrap_or_default();
                    alerts_col = alerts_col.push(
                        container(
                            row![
                                icons::lock(icon_size::XS, p.warning_fg),
                                theme::clipped_line(
                                    text(format!("Secrets vault locked{profile_tag}{key_hint}"))
                                        .size(type_scale::BADGE)
                                        .color(p.warning_fg),
                                    Length::Fill,
                                ),
                                tip(
                                    button(icons::trash(icon_size::XS, p.danger_fg))
                                        .padding([2, 6])
                                        .style(theme::subtle_btn(p))
                                        .on_press(Message::RequestConfirm(
                                            PendingConfirm::destroy_repo_vault(&repo_name),
                                        )),
                                    "Delete locked Secrets Vault (.gitfleet/secrets*.age & secrets.json)",
                                    tooltip::Position::Top,
                                    p,
                                ),
                                button(
                                    text("Keys")
                                        .size(type_scale::BADGE)
                                        .wrapping(text::Wrapping::None),
                                )
                                .padding([2, 8])
                                .style(theme::primary_btn(p))
                                .on_press(Message::OpenSecretsDrawer { show_keys: true }),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                } else if encrypt_needed > 0 {
                    has_alerts = true;
                    let paths = self.vault_summary.encrypt_needed_paths();
                    let label = match paths.as_slice() {
                        [only] => format!("Secrets modified{profile_tag}: {only}"),
                        [first, rest @ ..] => {
                            format!("Secrets modified{profile_tag}: {first} (+{} more)", rest.len())
                        }
                        [] => format!("{encrypt_needed} secret file(s) modified{profile_tag}"),
                    };
                    alerts_col = alerts_col.push(
                        container(
                            row![
                                icons::shield(icon_size::XS, p.warning_fg),
                                theme::clipped_line(
                                    text(label).size(type_scale::BADGE).color(p.warning_fg),
                                    Length::Fill,
                                ),
                                tip(
                                    button(
                                        row![
                                            icons::lock(10.0, Color::WHITE),
                                            text("Encrypt")
                                                .size(type_scale::BADGE)
                                                .wrapping(text::Wrapping::None),
                                        ]
                                        .spacing(4)
                                        .align_y(Alignment::Center),
                                    )
                                    .padding([2, 8])
                                    .style(theme::primary_btn(p))
                                    .on_press(Message::EncryptRepoEnv),
                                    format!(
                                        "Encrypt {} into .gitfleet/{vault_fname}",
                                        paths.join(", ")
                                    ),
                                    tooltip::Position::Top,
                                    p,
                                ),
                                button(
                                    text("Open")
                                        .size(type_scale::BADGE)
                                        .wrapping(text::Wrapping::None),
                                )
                                .padding([2, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenSecretsDrawer { show_keys: false }),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                } else if restore_needed > 0 {
                    has_alerts = true;
                    let paths = self.vault_summary.restore_needed_paths();
                    let label = match paths.as_slice() {
                        [only] => format!("Secret missing{profile_tag}: {only}"),
                        [first, rest @ ..] => {
                            format!("Secrets missing{profile_tag}: {first} (+{} more)", rest.len())
                        }
                        [] => format!("{restore_needed} secret file(s) missing{profile_tag}"),
                    };
                    alerts_col = alerts_col.push(
                        container(
                            row![
                                icons::shield(icon_size::XS, p.accent_fg),
                                theme::clipped_line(
                                    text(label).size(type_scale::BADGE).color(p.accent_fg),
                                    Length::Fill,
                                ),
                                tip(
                                    button(
                                        text("Restore")
                                            .size(type_scale::BADGE)
                                            .wrapping(text::Wrapping::None),
                                    )
                                    .padding([2, 8])
                                    .style(theme::primary_btn(p))
                                    .on_press(Message::RestoreRepoEnv),
                                    format!(
                                        "Decrypt {} from .gitfleet/{vault_fname}",
                                        paths.join(", ")
                                    ),
                                    tooltip::Position::Top,
                                    p,
                                ),
                                button(
                                    text("Open")
                                        .size(type_scale::BADGE)
                                        .wrapping(text::Wrapping::None),
                                )
                                .padding([2, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenSecretsDrawer { show_keys: false }),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(
                            p.fill_selected,
                            p.accent_fg,
                            p.border_default,
                        )),
                    );
                }

                let make_commit_btn = |fill: bool| -> Element<'_, Message> {
                    let inner: Element<'_, Message> = theme::clipped_line(
                        row![
                            icons::check(icon_size::XS, p.text_primary),
                            text("Commit")
                                .size(type_scale::CAPTION)
                                .color(p.text_primary)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                        Length::Shrink,
                    );
                    let content: Element<'_, Message> = if fill {
                        container(inner)
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .into()
                    } else {
                        inner
                    };
                    let mut btn = button(content)
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::CommitSubmitted);
                    if fill {
                        btn = btn.width(Length::Fill);
                    }
                    tip(
                        btn,
                        "Create commit from staged changes (Ctrl+Enter)",
                        tooltip::Position::Top,
                        p,
                    )
                };

                let make_commit_push_btn = |fill: bool| -> Element<'_, Message> {
                    let inner: Element<'_, Message> = theme::clipped_line(
                        row![
                            icons::push(icon_size::XS, Color::WHITE),
                            text("Commit & Push")
                                .size(type_scale::CAPTION)
                                .color(Color::WHITE)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                        Length::Shrink,
                    );
                    let content: Element<'_, Message> = if fill {
                        container(inner)
                            .width(Length::Fill)
                            .center_x(Length::Fill)
                            .into()
                    } else {
                        inner
                    };
                    let mut btn = button(content)
                        .padding([4, 9])
                        .style(theme::primary_btn(p))
                        .on_press(Message::CommitAndPushSubmitted);
                    if fill {
                        btn = btn.width(Length::Fill);
                    }
                    tip(
                        btn,
                        "Commit staged changes and push to remote (Ctrl+Shift+Enter)",
                        tooltip::Position::Top,
                        p,
                    )
                };

                let make_amend_checkbox = || -> Element<'_, Message> {
                    theme::clipped_line(
                        checkbox("Amend last commit", self.amend_commit)
                            .on_toggle(Message::AmendToggled)
                            .size(14)
                            .spacing(6)
                            .text_size(type_scale::CAPTION)
                            .style(theme::tree_checkbox(p)),
                        Length::Shrink,
                    )
                };

                // Wide (>= 355px): single crisp row — Amend on left, CTAs on right
                let wide_footer = row![
                    make_amend_checkbox(),
                    Space::with_width(Length::Fill),
                    make_commit_btn(false),
                    make_commit_push_btn(false),
                ]
                .spacing(space::XS)
                .width(Length::Fill)
                .align_y(Alignment::Center);

                // Medium (235..354px): Amend row + full-width balanced side-by-side CTAs
                let medium_footer = column![
                    make_amend_checkbox(),
                    row![
                        container(make_commit_btn(true)).width(Length::FillPortion(42)),
                        container(make_commit_push_btn(true)).width(Length::FillPortion(58)),
                    ]
                    .spacing(space::XS)
                    .width(Length::Fill)
                    .align_y(Alignment::Center),
                ]
                .spacing(space::XS)
                .width(Length::Fill);

                // Compact (< 235px): Amend lives in the Commit header; stack full-width CTAs
                // cleanly so "Commit & Push" stays crisp on a single line at any narrow width.
                let compact_footer = column![
                    make_commit_btn(true),
                    make_commit_push_btn(true),
                ]
                .spacing(space::XXS)
                .width(Length::Fill);

                let footer = theme::adaptive_width_3(
                    355.0,
                    235.0,
                    Length::Fill,
                    wide_footer,
                    medium_footer,
                    compact_footer,
                );

                let mut pane_col = column![
                    hdr,
                    summary_input,
                    cc_strip,
                    commit_box
                ]
                    .spacing(space::XXS)
                    .width(Length::Fill)
                    .height(Length::Fill);
                if has_alerts {
                    pane_col = pane_col.push(alerts_col.width(Length::Fill));
                }
                pane_col.push(footer).into()
            }
            GraphSelection::Commit(oid) => {
                if let Some(c) = self.commits.iter().find(|c| &c.oid == oid) {
                    let (initials, avatar_color) = theme::author_avatar_info(&c.author);
                    let big_avatar = container(
                        text(initials)
                            .size(15)
                            .color(Color::WHITE)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fixed(36.0))
                    .height(Length::Fixed(36.0))
                    .center_x(Length::Fixed(36.0))
                    .center_y(Length::Fixed(36.0))
                    .style(theme::avatar_box(avatar_color));

                    let parents_short: String = c
                        .parents
                        .iter()
                        .map(|poid| &poid[..poid.len().min(8)])
                        .collect::<Vec<_>>()
                        .join(", ");

                    let make_commit_id_row = || -> Element<'_, Message> {
                        theme::clipped_line(
                            row![
                                text("Commit")
                                    .size(type_scale::CAPTION)
                                    .color(p.text_muted)
                                    .wrapping(text::Wrapping::None),
                                text(c.short_oid.clone())
                                    .size(type_scale::CAPTION)
                                    .font(theme::CODE_FONT)
                                    .color(p.accent_fg)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                            Length::Fill,
                        )
                    };

                    let make_commit_detail_actions = || -> Element<'_, Message> {
                        row![
                            tip(
                                button(icons::cherry_pick(icon_size::SM, p.accent_fg))
                                    .padding([4, 5])
                                    .style(theme::vscode_icon_btn(p, false))
                                    .on_press(Message::OpenCherryPickModal {
                                        revisions: vec![c.oid.clone()],
                                        detail: format!("{}  {}", c.short_oid, c.summary),
                                    }),
                                "Cherry-Pick commit onto current branch…",
                                tooltip::Position::Bottom,
                                p,
                            ),
                            tip(
                                button(icons::revert(icon_size::SM, p.warning_fg))
                                    .padding([4, 5])
                                    .style(theme::vscode_icon_btn(p, false))
                                    .on_press(Message::RequestConfirm(
                                        PendingConfirm::revert_commit(
                                            c.oid.clone(),
                                            &c.short_oid,
                                            &c.summary,
                                        ),
                                    )),
                                "Revert commit (creates inverse commit)",
                                tooltip::Position::Bottom,
                                p,
                            ),
                            tip(
                                button(icons::tag(icon_size::SM, p.text_secondary))
                                    .padding([4, 5])
                                    .style(theme::vscode_icon_btn(p, false))
                                    .on_press(Message::CreateTagAtCommit(c.oid.clone())),
                                "Create Tag at this commit",
                                tooltip::Position::Bottom,
                                p,
                            ),
                            tip(
                                button(icons::reset_soft(icon_size::SM, p.text_secondary))
                                    .padding([4, 5])
                                    .style(theme::vscode_icon_btn(p, false))
                                    .on_press(Message::RequestConfirm(
                                        PendingConfirm::reset_commit(
                                            c.oid.clone(),
                                            &c.short_oid,
                                            ResetMode::Soft,
                                        ),
                                    )),
                                "Reset Soft to this commit (keep changes staged)",
                                tooltip::Position::Bottom,
                                p,
                            ),
                            tip(
                                button(icons::reset_mixed(icon_size::SM, p.danger_fg))
                                    .padding([4, 5])
                                    .style(theme::vscode_icon_btn(p, false))
                                    .on_press(Message::RequestConfirm(
                                        PendingConfirm::reset_commit(
                                            c.oid.clone(),
                                            &c.short_oid,
                                            ResetMode::Mixed,
                                        ),
                                    )),
                                "Reset Mixed to this commit (keep working tree)",
                                tooltip::Position::Bottom,
                                p,
                            ),
                        ]
                        .spacing(2)
                        .align_y(Alignment::Center)
                        .into()
                    };

                    // Wide (>= 255px): single header row; Narrow (< 255px): keep commit OID intact
                    // above a subtle recessed icon strip so actions never crush the hash.
                    let hdr = theme::adaptive_width(
                        255.0,
                        Length::Fill,
                        row![make_commit_id_row(), make_commit_detail_actions()]
                            .spacing(space::XXS)
                            .width(Length::Fill)
                            .align_y(Alignment::Center),
                        column![
                            make_commit_id_row(),
                            container(make_commit_detail_actions())
                                .padding([2, 4])
                                .width(Length::Fill)
                                .style(theme::table_col_header_box(p)),
                        ]
                        .spacing(space::XXS)
                        .width(Length::Fill),
                    );

                    let mut meta_col = column![
                        theme::clipped_line(
                            text(format!("by {}, {}", c.author, c.date_display))
                                .size(type_scale::CAPTION)
                                .color(p.text_secondary),
                            Length::Fill,
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fill);

                    if !parents_short.is_empty() {
                        meta_col = meta_col.push(theme::clipped_line(
                            row![
                                text("parent").size(type_scale::BADGE).color(p.text_muted),
                                text(parents_short)
                                    .size(type_scale::BADGE)
                                    .font(theme::CODE_FONT)
                                    .color(p.accent_fg),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                            Length::Fill,
                        ));
                    }

                    let mut msg_col = column![
                        text(c.summary.clone())
                            .size(type_scale::SUBHEADING)
                            .color(p.text_primary)
                            .width(Length::Fill),
                    ]
                    .spacing(6)
                    .width(Length::Fill);

                    if !c.body.is_empty() {
                        msg_col = msg_col.push(
                            text(c.body.clone())
                                .size(type_scale::CAPTION)
                                .color(p.text_secondary)
                                .width(Length::Fill),
                        );
                    }

                    column![
                        hdr,
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        row![big_avatar, meta_col]
                            .spacing(8)
                            .width(Length::Fill)
                            .align_y(Alignment::Center),
                        slim_scroll_h(msg_col, 240.0, p),
                    ]
                    .spacing(space::XS)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                } else {
                    theme::empty_state(
                        p,
                        icons::file_code(icon_size::MD, p.text_muted),
                        "No commit selected",
                        "Select a commit in the Graph to inspect its details.",
                        None,
                    )
                }
            }
        }
    }

    /// Resizable Top-Right-Bottom zone: SmartGit-style Files pane (`Changed | Dir | All`)
    /// with dedicated Unstaged + Staged split panels when inspecting Working Tree / Index.
    pub(crate) fn view_files_pane(&self, p: Palette) -> Element<'_, Message> {
        let active_status = self.active_status();
        let changed_count = self.active_changed_files().len();
        let multi_sel_count = if self.graph_selection == GraphSelection::WorkingTree {
            self.selected_changed_files.len()
        } else {
            0
        };

        let make_files_title = || -> Element<'_, Message> {
            let mut title_row = row![
                text("Files")
                    .size(type_scale::BODY)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None),
                theme::badge(
                    format!("{changed_count}"),
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center);
            if multi_sel_count > 1 {
                title_row = title_row.push(theme::badge(
                    format!("{multi_sel_count} selected"),
                    p.fill_selected,
                    p.accent_fg,
                    p.border_subtle,
                    false,
                ));
            }
            theme::clipped_line(title_row, Length::Fill)
        };

        let mode_seg_wide = |icon: Element<'static, Message>,
                             label: &'static str,
                             mode: FilesViewMode,
                             hint: &'static str|
         -> Element<'_, Message> {
            let active = self.files_mode == mode;
            tip(
                button(theme::clipped_line(
                    row![
                        icon,
                        text(label)
                            .size(type_scale::CAPTION)
                            .color(if active {
                                Color::WHITE
                            } else {
                                p.text_secondary
                            })
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                    Length::Shrink,
                ))
                .padding([3, 7])
                .style(theme::segmented_btn(p, active))
                .on_press(Message::SetFilesViewMode(mode)),
                hint,
                tooltip::Position::Bottom,
                p,
            )
        };

        let mode_seg_compact =
            |label: &'static str, mode: FilesViewMode, portion: u16, hint: &'static str| -> Element<'_, Message> {
                let active = self.files_mode == mode;
                container(tip(
                    button(
                        container(theme::clipped_line(
                            text(label)
                                .size(type_scale::CAPTION)
                                .color(if active {
                                    Color::WHITE
                                } else {
                                    p.text_secondary
                                })
                                .wrapping(text::Wrapping::None),
                            Length::Shrink,
                        ))
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                    )
                    .width(Length::Fill)
                    .padding([3, 4])
                    .style(theme::segmented_btn(p, active))
                    .on_press(Message::SetFilesViewMode(mode)),
                    hint,
                    tooltip::Position::Bottom,
                    p,
                ))
                .width(Length::FillPortion(portion))
                .into()
            };

        let wide_segmented = theme::segmented_group(
            p,
            row![
                mode_seg_wide(
                    icons::file_diff(
                        12.0,
                        if self.files_mode == FilesViewMode::Flat {
                            Color::WHITE
                        } else {
                            p.text_secondary
                        },
                    )
                    .into(),
                    "Changed",
                    FilesViewMode::Flat,
                    "Changed Files — Unstaged & Staged panels",
                ),
                mode_seg_wide(
                    icons::folder(
                        12.0,
                        if self.files_mode == FilesViewMode::Dir {
                            Color::WHITE
                        } else {
                            p.text_secondary
                        },
                    )
                    .into(),
                    "Dir",
                    FilesViewMode::Dir,
                    "Changed by Directory — Filter changed files by folder",
                ),
                mode_seg_wide(
                    icons::folder_tree(
                        12.0,
                        if self.files_mode == FilesViewMode::AllFiles {
                            Color::WHITE
                        } else {
                            p.text_secondary
                        },
                    )
                    .into(),
                    "All",
                    FilesViewMode::AllFiles,
                    "All Files — Browse all repository files & folders like VS Code (gitignored items dimmed)",
                ),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        );

        let compact_segmented = theme::segmented_group_fill(
            p,
            row![
                mode_seg_compact(
                    "Changed",
                    FilesViewMode::Flat,
                    44,
                    "Changed Files — Unstaged & Staged panels",
                ),
                mode_seg_compact(
                    "Dir",
                    FilesViewMode::Dir,
                    28,
                    "Changed by Directory — Filter changed files by folder",
                ),
                mode_seg_compact(
                    "All",
                    FilesViewMode::AllFiles,
                    28,
                    "All Files — Browse all repository files & folders like VS Code (gitignored items dimmed)",
                ),
            ]
            .spacing(2)
            .width(Length::Fill)
            .align_y(Alignment::Center),
        );

        // Wide (>= 305px): single header row (Files [N] + segmented switcher).
        // Compact (< 305px): clean 2-row header so title never vanishes and segments stay crisp.
        let top_bar = theme::adaptive_width(
            305.0,
            Length::Fill,
            row![make_files_title(), wide_segmented]
                .spacing(space::XS)
                .width(Length::Fill)
                .align_y(Alignment::Center),
            column![make_files_title(), compact_segmented]
                .spacing(space::XXS)
                .width(Length::Fill),
        );

        let search_msg = Message::OpenDetachedSearch(if self.file_filter.trim().is_empty() {
            None
        } else {
            Some(self.file_filter.trim().to_string())
        });
        let ignored_hint = if self.show_ignored_files {
            "Showing gitignored files & folders (dimmed like VS Code) — click to hide"
        } else {
            "Gitignored files & folders are hidden — click to show (e.g. .env)"
        };

        let mut wide_actions = row![].spacing(space::XXS).align_y(Alignment::Center);
        let mut compact_actions = row![].spacing(space::XXS).align_y(Alignment::Center);

        if self.files_mode == FilesViewMode::AllFiles {
            wide_actions = wide_actions.push(tip(
                button(theme::clipped_line(
                    row![
                        if self.show_ignored_files {
                            icons::eye(12.0, p.accent_fg)
                        } else {
                            icons::eye_off(12.0, p.text_muted)
                        },
                        text("Ignored")
                            .size(type_scale::CAPTION)
                            .color(if self.show_ignored_files {
                                p.accent_fg
                            } else {
                                p.text_muted
                            })
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                    Length::Shrink,
                ))
                .padding([4, 8])
                .style(theme::toolbar_btn(p, self.show_ignored_files))
                .on_press(Message::ToggleShowIgnoredFiles),
                ignored_hint,
                tooltip::Position::Bottom,
                p,
            ));

            compact_actions = compact_actions.push(tip(
                button(if self.show_ignored_files {
                    icons::eye(13.0, p.accent_fg)
                } else {
                    icons::eye_off(13.0, p.text_muted)
                })
                .padding([5, 6])
                .style(theme::toolbar_btn(p, self.show_ignored_files))
                .on_press(Message::ToggleShowIgnoredFiles),
                ignored_hint,
                tooltip::Position::Bottom,
                p,
            ));
        }

        wide_actions = wide_actions.push(tip(
            button(theme::clipped_line(
                row![
                    icons::folder_search(12.0, p.accent_fg),
                    text("Search")
                        .size(type_scale::CAPTION)
                        .color(p.text_primary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
                Length::Shrink,
            ))
            .padding([4, 8])
            .style(theme::subtle_btn(p))
            .on_press(search_msg.clone()),
            "VS Code-style Search & Replace in files (opens GitFleet Studio)",
            tooltip::Position::Bottom,
            p,
        ));

        compact_actions = compact_actions.push(tip(
            button(icons::folder_search(13.0, p.accent_fg))
                .padding([5, 6])
                .style(theme::subtle_btn(p))
                .on_press(search_msg),
            "Search & Replace in files (opens GitFleet Studio)",
            tooltip::Position::Bottom,
            p,
        ));

        // Keep `text_input` permanently at child index 0 so resizing never drops focus while
        // trailing buttons collapse smoothly to crisp icon buttons in narrow panes.
        let filter_actions_bp = if self.files_mode == FilesViewMode::AllFiles {
            315.0
        } else {
            245.0
        };
        let filter_row = row![
            text_input("Filter files…", &self.file_filter)
                .on_input(Message::FileFilterChanged)
                .padding([4, 8])
                .size(type_scale::CAPTION)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
            theme::adaptive_width(
                filter_actions_bp,
                Length::Shrink,
                wide_actions,
                compact_actions,
            ),
        ]
        .spacing(space::XS)
        .width(Length::Fill)
        .align_y(Alignment::Center);

        let files_header = column![top_bar, filter_row]
            .spacing(space::XS)
            .width(Length::Fill);

        let conflict_banner: Element<'_, Message> = if let Some(st) = active_status
            && (st.in_merge || st.in_rebase || st.conflict_count > 0)
        {
            let mode_name = if st.in_rebase { "Rebase" } else { "Merge" };
            container(
                row![
                    theme::clipped_line(
                        text(format!(
                            "{mode_name} in progress ({} conflicts)",
                            st.conflict_count
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.danger_fg),
                        Length::Fill,
                    ),
                    tip(
                        button(icons::play(icon_size::XS, Color::WHITE))
                            .padding([4, 7])
                            .style(theme::primary_btn(p))
                            .on_press(Message::ContinueGitOp),
                        "Continue Merge / Rebase",
                        tooltip::Position::Left,
                        p,
                    ),
                    tip(
                        button(icons::close_x(icon_size::XS, p.danger_fg))
                            .padding([4, 7])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::RequestConfirm(PendingConfirm::abort_git_op())),
                        "Abort Merge / Rebase",
                        tooltip::Position::Left,
                        p,
                    ),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .width(Length::Fill)
            .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg))
            .into()
        } else {
            Space::with_height(Length::Shrink).into()
        };

        let files_body: Element<'_, Message> = match self.files_mode {
            FilesViewMode::Flat => self.view_files_flat(p, None),
            FilesViewMode::Dir => self.view_files_by_dir(p),
            FilesViewMode::AllFiles => self.view_all_files_explorer(p),
        };

        column![conflict_banner, files_header, files_body]
            .spacing(space::XS)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// SmartGit-style Files view:
    /// - When inspecting `Working Tree / Index`:
    ///   - If there are NO staged files, renders a single calm full-height files table (Staged panel hidden).
    ///   - When at least one file is staged, splits cleanly into **Unstaged** (top) and **Staged** (bottom)
    ///     separated by a draggable splitter.
    /// - When inspecting a historical `Commit`: displays a single full-height commit files table.
    pub(crate) fn view_files_flat(
        &self,
        p: Palette,
        dir_filter: Option<&str>,
    ) -> Element<'_, Message> {
        let is_wt = self.graph_selection == GraphSelection::WorkingTree;

        if !is_wt {
            let visible_files = self.visible_files_for_section(false, dir_filter);
            return self.view_files_table(
                visible_files,
                false,
                "No changed files in this commit.",
                p,
            );
        }

        let unstaged_files = self.visible_files_for_section(false, dir_filter);
        let staged_files = self.visible_files_for_section(true, dir_filter);

        // Hide the Staged panel completely when there are no staged files so Unstaged gets 100% height.
        if staged_files.is_empty() {
            return self.view_files_table(
                unstaged_files,
                true,
                "Working tree clean.",
                p,
            );
        }

        let sel_unstaged_cnt = unstaged_files
            .iter()
            .filter(|f| self.selected_changed_files.contains(&(f.path.clone(), f.staged)))
            .count();
        let sel_staged_cnt = staged_files
            .iter()
            .filter(|f| self.selected_changed_files.contains(&(f.path.clone(), f.staged)))
            .count();

        let mut unstaged_hdr_row = row![
            container(Space::new(6, 6)).style(theme::dot_box(if unstaged_files.is_empty() {
                p.text_muted
            } else {
                p.warning_fg
            })),
            text("Unstaged")
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
            theme::badge(
                unstaged_files.len().to_string(),
                p.surface_card,
                if unstaged_files.is_empty() {
                    p.text_muted
                } else {
                    p.warning_fg
                },
                p.border_subtle,
                false,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if sel_unstaged_cnt > 1 {
            unstaged_hdr_row = unstaged_hdr_row.push(theme::badge(
                format!("{sel_unstaged_cnt} selected"),
                p.fill_selected,
                p.accent_fg,
                p.border_subtle,
                false,
            ));
        }

        let unstaged_hdr = container(theme::clipped_line(unstaged_hdr_row, Length::Fill))
            .padding([4, 8])
            .width(Length::Fill)
            .style(theme::section_header_box(p));

        let mut staged_hdr_row = row![
            container(Space::new(6, 6)).style(theme::dot_box(p.success_fg)),
            text("Staged")
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
            theme::badge(
                staged_files.len().to_string(),
                p.success_bg,
                p.success_fg,
                p.border_subtle,
                false,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if sel_staged_cnt > 1 {
            staged_hdr_row = staged_hdr_row.push(theme::badge(
                format!("{sel_staged_cnt} selected"),
                p.fill_selected,
                p.accent_fg,
                p.border_subtle,
                false,
            ));
        }

        let staged_hdr = container(theme::clipped_line(staged_hdr_row, Length::Fill))
            .padding([4, 8])
            .width(Length::Fill)
            .style(theme::section_header_box(p));

        let unstaged_panel = container(
            column![
                unstaged_hdr,
                self.view_files_table(
                    unstaged_files,
                    true,
                    "No unstaged changes.",
                    p,
                ),
            ]
            .spacing(2)
            .height(Length::Fill)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let staged_panel = container(
            column![
                staged_hdr,
                self.view_files_table(
                    staged_files,
                    true,
                    "No staged changes.",
                    p,
                ),
            ]
            .spacing(2)
            .height(Length::Fill)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let active_split = self.active_resize == Some(ResizeTarget::FilesStagedSplit);
        let splitter = mouse_area(
            container(Space::new(Length::Fill, Length::Fixed(5.0)))
                .width(Length::Fill)
                .height(Length::Fixed(5.0))
                .style(theme::pane_splitter(p, active_split)),
        )
        .interaction(mouse::Interaction::ResizingVertically)
        .on_press(Message::StartResize(ResizeTarget::FilesStagedSplit));

        let (top_p, bot_p) = split_portions(self.files_staged_split_ratio);

        column![
            unstaged_panel.height(Length::FillPortion(top_p)),
            splitter,
            staged_panel.height(Length::FillPortion(bot_p)),
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Reusable 3-column Files table (`Name | State | Directory`) with tight content-hugging
    /// column widths and horizontal scroll only when content truly overflows.
    fn view_files_table(
        &self,
        visible_files: Vec<FileChange>,
        is_wt: bool,
        empty_hint: &'static str,
        p: Palette,
    ) -> Element<'_, Message> {
        let mut max_name_w = theme::estimate_text_width("Name", type_scale::BADGE, false) + 8.0;
        let mut max_state_w = theme::estimate_text_width("State", type_scale::BADGE, false) + 8.0;
        let mut max_dir_w = theme::estimate_text_width("Directory", type_scale::BADGE, false) + 8.0;
        let mut has_conflict = false;
        let mut has_unstaged = false;
        let mut has_staged = false;

        for f in &visible_files {
            let fname = f.path.rsplit('/').next().unwrap_or(&f.path);
            let nw = 12.0 + 5.0 + theme::estimate_text_width(fname, type_scale::BODY, false) + 8.0;
            if nw > max_name_w {
                max_name_w = nw;
            }
            let sw =
                theme::estimate_text_width(f.kind.state_label(false), type_scale::CAPTION, false)
                    + 8.0;
            if sw > max_state_w {
                max_state_w = sw;
            }
            let d = parent_dir(&f.path);
            let d_disp = if d.is_empty() { "." } else { d };
            let dw = theme::estimate_text_width(d_disp, type_scale::CAPTION, false) + 8.0;
            if dw > max_dir_w {
                max_dir_w = dw;
            }
            if f.kind == FileChangeKind::Conflicted {
                has_conflict = true;
            } else if f.staged {
                has_staged = true;
            } else {
                has_unstaged = true;
            }
        }

        let name_w = max_name_w.clamp(68.0, 260.0);
        let state_w = max_state_w.clamp(48.0, 72.0);
        let dir_min_w = max_dir_w.clamp(56.0, 300.0);
        let actions_w = if !is_wt {
            0.0_f32
        } else if has_conflict {
            76.0_f32
        } else if has_unstaged {
            50.0_f32
        } else if has_staged {
            26.0_f32
        } else {
            0.0_f32
        };
        let table_min_w = name_w + state_w + dir_min_w + actions_w + 26.0;

        let mut col_hdr = row![
            theme::clipped_line(
                text("Name").size(type_scale::BADGE).color(p.text_muted),
                Length::Fixed(name_w),
            ),
            theme::clipped_line(
                text("State").size(type_scale::BADGE).color(p.text_muted),
                Length::Fixed(state_w),
            ),
            theme::clipped_line(
                text("Directory").size(type_scale::BADGE).color(p.text_muted),
                Length::Fill,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if actions_w > 0.0 {
            col_hdr = col_hdr.push(Space::with_width(Length::Fixed(actions_w)));
        }

        let header_strip = container(col_hdr)
            .padding([4, 7])
            .width(Length::Fill)
            .style(theme::table_col_header_box(p));

        if visible_files.is_empty() {
            return container(
                row![
                    icons::check(12.0, p.text_muted),
                    theme::clipped_line(
                        text(empty_hint)
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Length::Fill,
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([12, 10])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        }

        let mut list = column![].spacing(1);
        for file in visible_files {
            let file_dir = parent_dir(&file.path);
            let selected = self
                .selected_changed_files
                .contains(&(file.path.clone(), file.staged))
                || (self.selected_changed_files.is_empty()
                    && self.selected_changed_file.as_ref() == Some(&file));
            let (state_color, _) = file_state_colors(file.kind, p);
            let file_name = file.path.rsplit('/').next().unwrap_or(&file.path).to_string();
            let dir_display = if file_dir.is_empty() { "." } else { file_dir };
            // Show exact semantic change kind (`Added`, `Modified`, `Deleted`, `Renamed`, `Untracked`, `Conflict`)
            // since Unstaged and Staged now live in separate panels like SmartGit.
            let state_text = file.kind.state_label(false);

            let row_btn = theme::pressable_row(
                p,
                selected,
                iced::Padding::from([3, 7]),
                row![
                    theme::clipped_line(
                        row![
                            icons::file_code(12.0, state_color),
                            text(file_name)
                                .size(type_scale::BODY)
                                .color(p.text_primary),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                        Length::Fixed(name_w),
                    ),
                    theme::clipped_line(
                        text(state_text)
                            .size(type_scale::CAPTION)
                            .font(MEDIUM_FONT)
                            .color(state_color)
                            .wrapping(text::Wrapping::None),
                        Length::Fixed(state_w),
                    ),
                    theme::clipped_line(
                        text(dir_display.to_string())
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Length::Fill,
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                Message::ClickChangedFile(file.clone()),
            );

            let mut line = row![row_btn].spacing(3).align_y(Alignment::Center);

            if is_wt {
                if file.kind == FileChangeKind::Conflicted {
                    line = line
                        .push(tip(
                            button(text("O").size(type_scale::BADGE))
                                .padding([3, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::ResolveConflict(
                                    file.path.clone(),
                                    ConflictStrategy::Ours,
                                )),
                            "Keep Ours",
                            tooltip::Position::Left,
                            p,
                        ))
                        .push(tip(
                            button(text("T").size(type_scale::BADGE))
                                .padding([3, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::ResolveConflict(
                                    file.path.clone(),
                                    ConflictStrategy::Theirs,
                                )),
                            "Keep Theirs",
                            tooltip::Position::Left,
                            p,
                        ))
                        .push(tip(
                            button(icons::check(11.0, Color::WHITE))
                                .padding([4, 6])
                                .style(theme::primary_btn(p))
                                .on_press(Message::ResolveConflict(
                                    file.path.clone(),
                                    ConflictStrategy::MarkResolved,
                                )),
                            "Mark Conflict Resolved",
                            tooltip::Position::Left,
                            p,
                        ));
                } else if file.staged {
                    line = line.push(tip(
                        button(icons::minus(11.0, p.warning_fg))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::UnstageFile(file.path.clone())),
                        "Unstage this file",
                        tooltip::Position::Left,
                        p,
                    ));
                } else {
                    line = line
                        .push(tip(
                            button(icons::plus(11.0, p.success_fg))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::StageFile(file.path.clone())),
                            "Stage this file",
                            tooltip::Position::Left,
                            p,
                        ))
                        .push(tip(
                            button(icons::undo(11.0, p.danger_fg))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenDiscardModal(Some(vec![file.path.clone()]))),
                            "Discard changes in this file… (Ctrl+Z)",
                            tooltip::Position::Left,
                            p,
                        ));
                }
            }

            list = list.push(mouse_area(line).on_right_press(Message::OpenContextMenu(
                ContextMenuTarget::ChangedFile {
                    file: file.clone(),
                    is_working_tree: is_wt,
                },
            )));
        }

        let table = column![header_strip, list].spacing(2).width(Length::Fill);
        slim_scroll_h(table, table_min_w, p)
    }

    /// Dir mode: Left column lists directories containing changes; clicking filters the right column.
    pub(crate) fn view_files_by_dir(&self, p: Palette) -> Element<'_, Message> {
        let files = self.active_changed_files();
        let mut dir_counts: BTreeMap<String, usize> = BTreeMap::new();
        for f in &files {
            *dir_counts
                .entry(parent_dir(&f.path).to_string())
                .or_insert(0) += 1;
        }

        let mut dirs_col = column![
            button(theme::clipped_line(
                row![
                    icons::folder(12.0, p.accent_fg),
                    text(format!("All dirs ({})", files.len()))
                        .size(type_scale::CAPTION)
                        .color(p.text_primary),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
                Length::Fill,
            ))
            .width(Length::Fill)
            .padding([4, 6])
            .style(theme::repo_row_btn(p, self.selected_dir_filter.is_none()))
            .on_press(Message::SelectDirFilter(None)),
        ]
        .spacing(2);

        let all_dirs_label = format!("All dirs ({})", files.len());
        let mut max_dir_w: f32 =
            12.0 + 12.0 + 5.0 + theme::estimate_text_width(&all_dirs_label, type_scale::CAPTION, false) + 8.0;
        for (dir, cnt) in dir_counts {
            let is_sel = self.selected_dir_filter.as_deref() == Some(dir.as_str());
            let label = if dir.is_empty() {
                "(root)".to_string()
            } else {
                dir.clone()
            };
            let cnt_str = format!("({cnt})");
            let est_w = 12.0
                + 12.0
                + 5.0
                + theme::estimate_text_width(&label, type_scale::CAPTION, false)
                + 5.0
                + theme::estimate_text_width(&cnt_str, type_scale::BADGE, false)
                + 8.0;
            if est_w > max_dir_w {
                max_dir_w = est_w;
            }
            dirs_col = dirs_col.push(
                button(theme::clipped_line(
                    row![
                        icons::folder(12.0, p.warning_fg),
                        text(label).size(type_scale::CAPTION).color(p.text_primary),
                        text(cnt_str)
                            .size(type_scale::BADGE)
                            .color(p.warning_fg),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                    Length::Fill,
                ))
                .width(Length::Fill)
                .padding([4, 6])
                .style(theme::repo_row_btn(p, is_sel))
                .on_press(Message::SelectDirFilter(Some(dir))),
            );
        }

        let left_dirs = container(slim_scroll_h(dirs_col, max_dir_w.clamp(88.0, 480.0), p))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(4)
            .style(theme::section_panel_box(p));

        let right_files = container(self.view_files_flat(p, self.selected_dir_filter.as_deref()))
            .width(Length::Fill)
            .height(Length::Fill);

        theme::responsive_split(290.0, 0.38, 6.0, left_dirs, right_files)
    }

    /// All Files mode: Left = collapsible folder tree, Right = files in selected folder.
    /// Shows all repository files & folders like VS Code, with gitignored items dimmed.
    pub(crate) fn view_all_files_explorer(&self, p: Palette) -> Element<'_, Message> {
        let q = self.file_filter.to_lowercase();

        let mut dirty_map: HashMap<&str, (FileChangeKind, bool)> = HashMap::new();
        if let Some(st) = self.active_status() {
            for f in &st.staged_files {
                dirty_map.insert(&f.path, (f.kind, true));
            }
            for f in &st.unstaged_files {
                dirty_map.insert(&f.path, (f.kind, false));
            }
            for f in &st.conflict_files {
                dirty_map.insert(&f.path, (f.kind, false));
            }
        }

        let mut root_tree = FolderTreeNode::default();
        for f in &self.all_repo_files {
            root_tree.insert_file(f, dirty_map.contains_key(f.as_str()), false);
        }
        if self.show_ignored_files {
            for f in &self.ignored_repo_files {
                root_tree.insert_file(f, false, true);
            }
            for d in &self.ignored_repo_dirs {
                root_tree.insert_ignored_dir(d, &self.ignored_dir_children);
            }
        }

        let root_label = self
            .current_repo()
            .map(|r| r.name)
            .unwrap_or_else(|| "(root)".to_string());
        let tree_ctx = FolderTreeCtx {
            selected_dir: &self.selected_all_dir,
            expanded_dirs: &self.expanded_all_dirs,
            p,
        };
        let mut dir_rows: Vec<Element<'_, Message>> = Vec::new();
        let mut max_tree_w: f32 = 90.0;
        render_folder_tree(
            &root_tree,
            &root_label,
            "",
            0,
            &tree_ctx,
            &mut dir_rows,
            &mut max_tree_w,
        );

        let mut ignored_dir_loading = false;
        let mut ignored_dir_truncated = false;
        let mut ignored_dir_has_subdirs = false;

        let (visible_files, recursive_prefix): (Vec<(&str, bool)>, Option<String>) = if !q.is_empty()
        {
            let mut matched: Vec<(&str, bool)> = self
                .all_repo_files
                .iter()
                .filter(|f| f.to_lowercase().contains(&q))
                .map(|f| (f.as_str(), false))
                .collect();
            if self.show_ignored_files {
                let mut seen: HashSet<&str> = matched.iter().map(|(f, _)| *f).collect();
                for f in &self.ignored_repo_files {
                    if f.to_lowercase().contains(&q) && seen.insert(f.as_str()) {
                        matched.push((f.as_str(), true));
                    }
                }
                for listing in self.ignored_dir_children.values() {
                    for f in &listing.files {
                        if f.to_lowercase().contains(&q) && seen.insert(f.as_str()) {
                            matched.push((f.as_str(), true));
                        }
                    }
                }
            }
            matched.sort_by(|a, b| a.0.cmp(b.0));
            (matched, None)
        } else if self.show_ignored_files && self.is_path_in_ignored_dir(&self.selected_all_dir) {
            if let Some(listing) = self.ignored_dir_children.get(&self.selected_all_dir) {
                ignored_dir_truncated = listing.truncated;
                ignored_dir_has_subdirs = !listing.dirs.is_empty();
                (
                    listing.files.iter().map(|f| (f.as_str(), true)).collect(),
                    None,
                )
            } else {
                ignored_dir_loading = true;
                (Vec::new(), None)
            }
        } else {
            let mut direct: Vec<(&str, bool)> = self
                .all_repo_files
                .iter()
                .filter(|f| parent_dir(f) == self.selected_all_dir)
                .map(|f| (f.as_str(), false))
                .collect();
            if self.show_ignored_files {
                for f in &self.ignored_repo_files {
                    if parent_dir(f) == self.selected_all_dir {
                        direct.push((f.as_str(), true));
                    }
                }
            }
            direct.sort_by(|a, b| a.0.cmp(b.0));

            if direct.is_empty() && !self.selected_all_dir.is_empty() {
                let prefix = format!("{}/", self.selected_all_dir);
                let mut nested: Vec<(&str, bool)> = self
                    .all_repo_files
                    .iter()
                    .filter(|f| f.starts_with(&prefix))
                    .map(|f| (f.as_str(), false))
                    .collect();
                if self.show_ignored_files {
                    for f in &self.ignored_repo_files {
                        if f.starts_with(&prefix) {
                            nested.push((f.as_str(), true));
                        }
                    }
                }
                nested.sort_by(|a, b| a.0.cmp(b.0));
                (nested, Some(prefix))
            } else {
                (direct, None)
            }
        };

        let mut files_col = column![].spacing(2);
        let mut max_file_w: f32 = 96.0;

        if ignored_dir_loading {
            files_col = files_col.push(
                container(
                    text("Loading folder…")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([8, 8]),
            );
        } else if visible_files.is_empty() {
            let empty_msg = if ignored_dir_has_subdirs {
                "Expand folder on the left to browse subfolders."
            } else if !q.is_empty() {
                "No matching files."
            } else {
                "Empty folder."
            };
            files_col = files_col.push(
                container(
                    text(empty_msg)
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([8, 8]),
            );
        } else {
            for (f, is_ignored) in visible_files {
                let active = self.active_file_path.as_deref() == Some(f);
                let name = if !q.is_empty() {
                    f.to_string()
                } else if let Some(prefix) = &recursive_prefix {
                    f.strip_prefix(prefix).unwrap_or(f).to_string()
                } else {
                    f.rsplit('/').next().unwrap_or(f).to_string()
                };
                let status_info = dirty_map.get(f).copied().map(|(k, _staged)| {
                    let (fg, bg) = file_state_colors(k, p);
                    (k.short_code(), fg, bg)
                });
                let badge_w = status_info
                    .map(|(code, _, _)| {
                        6.0 + theme::estimate_text_width(code, type_scale::BADGE, false) + 12.0
                    })
                    .unwrap_or(0.0);
                let est_w = 14.0
                    + 12.0
                    + 5.0
                    + theme::estimate_text_width(&name, type_scale::BODY, false)
                    + badge_w
                    + 8.0;
                if est_w > max_file_w {
                    max_file_w = est_w;
                }

                let icon_color = if let Some((_, fg, _)) = status_info {
                    fg
                } else if is_ignored {
                    Color { a: 0.56, ..p.text_muted }
                } else {
                    p.text_muted
                };
                let name_color = if let Some((_, fg, _)) = status_info {
                    fg
                } else if is_ignored {
                    if active {
                        p.text_secondary
                    } else {
                        Color { a: 0.72, ..p.text_muted }
                    }
                } else {
                    p.text_primary
                };

                let mut file_inner = row![
                    icons::file_code(12.0, icon_color),
                    text(name).size(type_scale::BODY).color(name_color),
                ]
                .spacing(5)
                .align_y(Alignment::Center);
                if let Some((code, fg, bg)) = status_info {
                    let border = Color { a: 0.42, ..fg };
                    file_inner = file_inner.push(
                        container(
                            text(code)
                                .size(type_scale::BADGE)
                                .color(fg)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding([1, 5])
                        .style(theme::pill_box(bg, fg, border)),
                    );
                }

                let file_btn = theme::pressable_row(
                    p,
                    active,
                    iced::Padding::from([4, 7]),
                    theme::clipped_line(file_inner, Length::Fill),
                    Message::ClickRepoFile(f.to_string()),
                );

                files_col = files_col.push(
                    mouse_area(file_btn).on_right_press(Message::OpenContextMenu(
                        ContextMenuTarget::RepoFile { path: f.to_string() },
                    )),
                );
            }

            if ignored_dir_truncated {
                files_col = files_col.push(
                    container(
                        text("Showing first 500 entries in this ignored folder.")
                            .size(type_scale::BADGE)
                            .color(p.text_muted),
                    )
                    .padding([6, 7]),
                );
            }
        }

        let left_dirs = container(slim_scroll_h(
            column(dir_rows).spacing(1),
            max_tree_w.clamp(90.0, 560.0),
            p,
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(4)
        .style(theme::section_panel_box(p));

        let right_files = container(slim_scroll_h(files_col, max_file_w.clamp(96.0, 560.0), p))
            .width(Length::Fill)
            .height(Length::Fill);

        theme::responsive_split(290.0, 0.42, 6.0, left_dirs, right_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nested_all_files_folder_tree_with_counts_and_dirty_flags() {
        let mut root = FolderTreeNode::default();
        root.insert_file("Cargo.toml", false, false);
        root.insert_file("crates/gitfleet-ui/src/main.rs", false, false);
        root.insert_file("crates/gitfleet-ui/src/views/commit_files.rs", true, false);
        root.insert_file("crates/gitfleet-git/src/lib.rs", false, false);

        assert_eq!(root.total_files, 4);
        assert!(root.has_dirty);
        assert!(!root.is_ignored_only());

        let crates = &root.children["crates"];
        assert_eq!(crates.total_files, 3);
        assert!(crates.has_dirty);
        assert!(!crates.is_ignored_only());

        let ui = &crates.children["gitfleet-ui"];
        assert_eq!(ui.total_files, 2);
        assert!(ui.has_dirty);
        assert_eq!(ui.children["src"].children["views"].total_files, 1);
        assert!(ui.children["src"].children["views"].has_dirty);

        let git = &crates.children["gitfleet-git"];
        assert_eq!(git.total_files, 1);
        assert!(!git.has_dirty);

        let mut expanded = HashSet::new();
        assert!(is_folder_expanded("", &expanded));
        assert!(!is_folder_expanded("crates", &expanded));
        expanded.insert(String::new());
        expanded.insert("crates".to_string());
        assert!(!is_folder_expanded("", &expanded));
        assert!(is_folder_expanded("crates", &expanded));
    }

    #[test]
    fn marks_ignored_only_and_lazy_ignored_folders_correctly_like_vscode() {
        let mut root = FolderTreeNode::default();
        root.insert_file("crates/app/main.rs", false, false);
        // Ignored file inside tracked directory -> parent stays non-ignored
        root.insert_file("crates/app/.env", false, true);
        // Ignored file inside ignored-only directory -> folder is dimmed
        root.insert_file("local_only/.env", false, true);

        let mut cache = HashMap::new();
        cache.insert(
            "target".to_string(),
            gitfleet_git::IgnoredDirListing {
                files: vec!["target/CACHEDIR.TAG".into()],
                dirs: vec!["target/debug".into()],
                truncated: false,
            },
        );
        root.insert_ignored_dir(".vscode", &cache);
        root.insert_ignored_dir("target", &cache);

        assert!(!root.children["crates"].is_ignored_only());
        assert!(!root.children["crates"].children["app"].is_ignored_only());
        assert!(root.children["local_only"].is_ignored_only());

        let vscode = &root.children[".vscode"];
        assert!(vscode.is_ignored_only());
        assert!(vscode.lazy_ignored_dir);
        assert!(!vscode.lazy_loaded);

        let target = &root.children["target"];
        assert!(target.is_ignored_only());
        assert!(target.lazy_ignored_dir);
        assert!(target.lazy_loaded);
        assert_eq!(target.total_files, 1);
        assert!(target.children["debug"].is_ignored_only());
        assert!(target.children["debug"].lazy_ignored_dir);
        assert!(!target.children["debug"].lazy_loaded);
    }

    #[test]
    fn multi_file_selection_supports_shift_range_and_ctrl_toggle() {
        let mk = |p: &str, staged: bool| FileChange {
            path: p.to_string(),
            kind: FileChangeKind::Untracked,
            staged,
        };
        let visible = vec![
            mk("a.rs", false),
            mk("b.rs", false),
            mk("c.rs", false),
            mk("d.rs", false),
            mk("e.rs", false),
        ];
        let mut sel = HashSet::new();
        let mut anchor = None;

        // 1. Plain click selects b.rs as anchor
        apply_file_selection(&mut sel, &mut anchor, &visible, &visible[1], false, false);
        assert_eq!(sel.len(), 1);
        assert!(sel.contains(&("b.rs".into(), false)));

        // 2. Shift+Click on d.rs selects b.rs..=d.rs (3 files), preserving b.rs as anchor
        apply_file_selection(&mut sel, &mut anchor, &visible, &visible[3], true, false);
        assert_eq!(sel.len(), 3);
        assert!(sel.contains(&("b.rs".into(), false)));
        assert!(sel.contains(&("c.rs".into(), false)));
        assert!(sel.contains(&("d.rs".into(), false)));

        // 3. Reverse Shift+Click on a.rs adjusts range to a.rs..=b.rs (2 files)
        apply_file_selection(&mut sel, &mut anchor, &visible, &visible[0], true, false);
        assert_eq!(sel.len(), 2);
        assert!(sel.contains(&("a.rs".into(), false)));
        assert!(sel.contains(&("b.rs".into(), false)));

        // 4. Ctrl+Click toggles e.rs in and a.rs out
        apply_file_selection(&mut sel, &mut anchor, &visible, &visible[4], false, true);
        assert_eq!(sel.len(), 3);
        assert!(sel.contains(&("e.rs".into(), false)));
        apply_file_selection(&mut sel, &mut anchor, &visible, &visible[0], false, true);
        assert_eq!(sel.len(), 2);
        assert!(!sel.contains(&("a.rs".into(), false)));
    }
}
