//! Pure editor buffer helpers, local file/compare payload loaders, background task spawners,
//! and diff message forwarding for GitFleet Studio.

use super::state::{DetachedAction, DetachedComparePayload, DetachedViewMode};
use crate::app::Message;
use crate::views::csv_diff::{compute_csv_diff, is_csv_diff_path};
use crate::views::image_diff::{
    decode_image_diff_pair, is_image_diff_path, is_raster_image_path, is_svg_path,
};
use gitfleet_docs::{detect_and_decode, detect_line_ending};
use gitfleet_git::{FileVersionPair, ParsedDiff, diff_two_texts};
use iced::widget::text_editor;
use iced::{Task, window};
use std::path::{Path, PathBuf};

// ── Helpers ──────────────────────────────────────────────────────────────────

pub(crate) fn editor_buffer_text(content: &text_editor::Content) -> String {
    let mut out = String::new();
    for (idx, line) in content.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&line);
    }
    out
}

pub(crate) fn format_editor_text_for_save(
    content: &text_editor::Content,
    trailing_newline: bool,
) -> String {
    let mut out = editor_buffer_text(content);
    if trailing_newline && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub(crate) fn jump_editor_to_span(
    editor: &mut text_editor::Content,
    line_index: usize,
    col_start: usize,
    col_end: usize,
) {
    use text_editor::{Action, Motion};
    editor.perform(Action::Move(Motion::DocumentStart));
    let max_steps = editor.line_count().saturating_mul(2).max(line_index + 8);
    for _ in 0..max_steps {
        let (cur_line, _) = editor.cursor_position();
        if cur_line >= line_index {
            break;
        }
        let prev = editor.cursor_position();
        editor.perform(Action::Move(Motion::Down));
        if editor.cursor_position() == prev {
            break;
        }
    }
    editor.perform(Action::Move(Motion::Home));
    for _ in 0..col_start {
        let prev = editor.cursor_position();
        editor.perform(Action::Move(Motion::Right));
        let cur = editor.cursor_position();
        if cur.0 != line_index || cur == prev {
            break;
        }
    }
    let span = col_end.saturating_sub(col_start);
    for _ in 0..span {
        let prev = editor.cursor_position();
        editor.perform(Action::Select(Motion::Right));
        if editor.cursor_position() == prev {
            break;
        }
    }
}

pub(crate) fn file_display_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn load_single_file_payload(path: PathBuf) -> Result<DetachedComparePayload, String> {
    let name = file_display_name(&path);
    let is_raster = is_raster_image_path(&name);
    let (preview_text, editable_source) = if is_raster {
        (None, None)
    } else {
        let bytes =
            std::fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        match detect_and_decode(&bytes) {
            Ok((encoding, decoded)) => {
                let eol = detect_line_ending(&decoded);
                let normalized = decoded.replace("\r\n", "\n");
                (
                    Some(normalized.clone()),
                    Some((path.clone(), normalized, encoding, eol)),
                )
            }
            Err(_) => (None, None),
        }
    };

    let preferred_mode = if editable_source.is_some() {
        DetachedViewMode::Edit
    } else {
        DetachedViewMode::Preview
    };

    Ok(DetachedComparePayload {
        title: name,
        left_path: None,
        right_path: None,
        single_file_path: Some(path.clone()),
        parsed_diff: ParsedDiff::default(),
        image_diff: None,
        csv_diff: None,
        preview_source: Some((path.clone(), preview_text)),
        editable_source,
        preferred_mode,
        status_note: format!("Opened {}", path.display()),
    })
}

pub(crate) fn compare_local_files_payload(
    left: PathBuf,
    right: PathBuf,
    preferred_text_mode: DetachedViewMode,
) -> Result<DetachedComparePayload, String> {
    let left_bytes = std::fs::read(&left)
        .map_err(|e| format!("Cannot read left file {}: {e}", left.display()))?;
    let right_bytes = std::fs::read(&right)
        .map_err(|e| format!("Cannot read right file {}: {e}", right.display()))?;

    let left_name = file_display_name(&left);
    let right_name = file_display_name(&right);
    let title = format!("{left_name} ↔ {right_name}");

    let is_img = is_image_diff_path(&left_name) || is_image_diff_path(&right_name);
    let is_raster = is_raster_image_path(&left_name) || is_raster_image_path(&right_name);
    let is_svg = is_svg_path(&left_name) && is_svg_path(&right_name);
    let is_csv = is_csv_diff_path(&left_name) || is_csv_diff_path(&right_name);

    let mut image_diff = None;
    let mut csv_diff = None;
    let mut parsed_diff = ParsedDiff::default();

    let representative_path = if is_image_diff_path(&right_name) || is_csv_diff_path(&right_name) {
        right_name.clone()
    } else {
        left_name.clone()
    };

    if is_img {
        image_diff = Some(decode_image_diff_pair(FileVersionPair {
            file_path: representative_path.clone(),
            before_label: left_name.clone(),
            after_label: right_name.clone(),
            before_bytes: Some(left_bytes.clone()),
            after_bytes: Some(right_bytes.clone()),
        }));
    }

    if is_csv {
        csv_diff = Some(compute_csv_diff(FileVersionPair {
            file_path: representative_path,
            before_label: left_name.clone(),
            after_label: right_name.clone(),
            before_bytes: Some(left_bytes.clone()),
            after_bytes: Some(right_bytes.clone()),
        }));
    }

    let left_decoded = detect_and_decode(&left_bytes).ok();
    let right_decoded = detect_and_decode(&right_bytes).ok();

    let editable_source = if !is_raster {
        right_decoded.as_ref().map(|(enc, dec)| {
            let eol = detect_line_ending(dec);
            (right.clone(), dec.replace("\r\n", "\n"), *enc, eol)
        })
    } else {
        None
    };

    let right_text = editable_source.as_ref().map(|(_, s, _, _)| s.clone());

    if (!is_raster || is_svg)
        && let (Some((_, l_str)), Some((_, r_str))) = (&left_decoded, &right_decoded)
    {
        parsed_diff = diff_two_texts(&left_name, l_str, &right_name, r_str);
    } else if !is_img {
        parsed_diff = ParsedDiff {
            raw: format!(
                "Binary files differ or cannot be displayed as text:\n  Left:  {} ({} bytes)\n  Right: {} ({} bytes)",
                left.display(),
                left_bytes.len(),
                right.display(),
                right_bytes.len(),
            ),
            ..Default::default()
        };
    }

    let preferred_mode = if is_img {
        DetachedViewMode::ImageDiff
    } else if is_csv {
        DetachedViewMode::CsvDiff
    } else {
        preferred_text_mode
    };

    Ok(DetachedComparePayload {
        title,
        left_path: Some(left.clone()),
        right_path: Some(right.clone()),
        single_file_path: None,
        parsed_diff,
        image_diff,
        csv_diff,
        preview_source: Some((right.clone(), right_text)),
        editable_source,
        preferred_mode,
        status_note: format!("Comparing {left_name} ↔ {right_name}"),
    })
}

pub(crate) fn spawn_compare_task(
    win_id: window::Id,
    req_id: u64,
    left: PathBuf,
    right: PathBuf,
    preferred_text_mode: DetachedViewMode,
) -> Task<Message> {
    Task::perform(
        async move {
            let res = tokio::task::spawn_blocking(move || {
                compare_local_files_payload(left, right, preferred_text_mode)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Comparison task failed: {e}")));
            (req_id, res)
        },
        move |(id, res)| match res {
            Ok(payload) => {
                Message::Detached(win_id, DetachedAction::PayloadLoaded(id, Box::new(payload)))
            }
            Err(err) => Message::Detached(win_id, DetachedAction::LoadFailed(id, err)),
        },
    )
}

pub(crate) fn spawn_single_file_task(
    win_id: window::Id,
    req_id: u64,
    path: PathBuf,
) -> Task<Message> {
    Task::perform(
        async move {
            let res = tokio::task::spawn_blocking(move || load_single_file_payload(path))
                .await
                .unwrap_or_else(|e| Err(format!("File load task failed: {e}")));
            (req_id, res)
        },
        move |(id, res)| match res {
            Ok(payload) => {
                Message::Detached(win_id, DetachedAction::PayloadLoaded(id, Box::new(payload)))
            }
            Err(err) => Message::Detached(win_id, DetachedAction::LoadFailed(id, err)),
        },
    )
}

pub(crate) fn map_text_diff_msg(win_id: window::Id, msg: Message) -> Message {
    match msg {
        Message::ExpandDiffGap { gap_id, mode } => {
            Message::Detached(win_id, DetachedAction::ExpandGap { gap_id, mode })
        }
        Message::DiffLeftScrolled(vp) => {
            Message::Detached(win_id, DetachedAction::LeftScrolled(vp))
        }
        Message::DiffRightScrolled(vp) => {
            Message::Detached(win_id, DetachedAction::RightScrolled(vp))
        }
        Message::DiffUnifiedScrolled(vp) => {
            Message::Detached(win_id, DetachedAction::UnifiedScrolled(vp))
        }
        Message::DiffSelStart { side, row, col } => {
            Message::Detached(win_id, DetachedAction::SelStart { side, row, col })
        }
        Message::DiffSelDrag { side, row, col } => {
            Message::Detached(win_id, DetachedAction::SelDrag { side, row, col })
        }
        Message::PreviewEditorAction(action) => {
            Message::Detached(win_id, DetachedAction::PreviewEditorAction(action))
        }
        Message::PreviewLinkClicked(url) => {
            Message::Detached(win_id, DetachedAction::PreviewLinkClicked(url))
        }
        _ => Message::Noop,
    }
}
