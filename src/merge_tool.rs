use std::{
    collections::HashMap,
    env, fs,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow};
use eframe::egui::containers::scroll_area::ScrollBarVisibility;
use eframe::{
    App,
    egui::{
        self, Align, Align2, Color32, ComboBox, FontId, Layout, Pos2, Rect, RichText, ScrollArea,
        Sense, Ui, Vec2,
    },
};

use crate::dialog;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeTheme {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeLanguage {
    English,
    Chinese,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeIgnoreMode {
    None,
    TrimWhitespace,
    IgnoreWhitespace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeHighlightMode {
    Lines,
    Words,
}

#[derive(Clone, Copy)]
enum MergeToolbarToggleIcon {
    Collapse,
    Expand,
    Sun,
    Moon,
    Language,
}

#[derive(Clone, Debug)]
struct MergeSourceText {
    base: String,
    local: String,
    remote: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeArgs {
    pub base: PathBuf,
    pub local: PathBuf,
    pub remote: PathBuf,
    pub output: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub stage: bool,
    pub theme: MergeTheme,
    pub language: MergeLanguage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeDocument {
    pub lines: Vec<MergeLine>,
    conflicts: Vec<ConflictBlock>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeLine {
    pub base: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub result: String,
    pub include_in_result: bool,
    pub kind: MergeLineKind,
    pub conflict_index: Option<usize>,
    local_resolved: bool,
    remote_resolved: bool,
    local_taken: bool,
    remote_taken: bool,
    base_only_resolved: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeLineKind {
    Resolved,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictBlock {
    pub index: usize,
    pub base: Vec<String>,
    pub local: Vec<String>,
    pub remote: Vec<String>,
    line_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeSide {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeLineAction {
    Take,
    Drop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeLineActionTarget {
    Conflict(usize),
    BaseOnlyGroup(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeSideLineTone {
    Unchanged,
    Added,
    BaseOnly,
    Deleted,
    Replaced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeConnectorDebug {
    Off,
    Guides,
    Log,
}

pub const MERGE_TOOL_CANCEL_EXIT_CODE: i32 = 10;

const MERGE_NAV_BUTTON_SIZE: f32 = 18.0;
const MERGE_PANEL_RADIUS: u8 = 6;
const MERGE_CODE_ROW_HEIGHT: f32 = 18.0;
const MERGE_CODE_FONT_SIZE: f32 = 12.0;
const MERGE_WORD_BLOCK_OPACITY: f32 = 1.0;
const MERGE_WORD_ACTIVE_BLOCK_OPACITY: f32 = 1.0;
const MERGE_BASE_ONLY_MARKER_HEIGHT: f32 = 3.0;
const MERGE_VIRTUAL_ROW_THRESHOLD: usize = 2_000;
const MERGE_COLLAPSE_MIN_UNCHANGED_ROWS: usize = 12;
const MERGE_COLLAPSE_CONTEXT_ROWS: usize = 3;
const MERGE_BUILD_CONFIG: &str = include_str!("../config/merge-tool.toml");
static MERGE_CONNECTOR_DEBUG_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
struct MergePalette {
    bg: Color32,
    panel: Color32,
    panel_soft: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
    conflict_fill: Color32,
    active_conflict_fill: Color32,
    conflict_text: Color32,
    added_fill: Color32,
    added_text: Color32,
    base_only_fill: Color32,
    base_only_connector_fill: Color32,
    base_only_text: Color32,
    connector: Color32,
    result_fill: Color32,
    shadow: eframe::epaint::Shadow,
}

#[derive(Default)]
struct MergePanelGeometry {
    rows: Vec<(usize, Rect)>,
}

impl MergePanelGeometry {
    fn record_row(&mut self, index: usize, rect: Rect) {
        self.rows.push((index, rect));
    }

    fn span_rect(&self, first: usize, count: usize) -> Option<Rect> {
        let last = first.saturating_add(count);
        self.rows
            .iter()
            .filter(|(index, _)| *index >= first && *index < last)
            .map(|(_, rect)| *rect)
            .reduce(|merged, rect| merged.union(rect))
    }

    fn boundary_marker_rect(&self, row_index: usize, height: f32) -> Option<Rect> {
        let next = self
            .rows
            .iter()
            .find(|(index, _)| *index == row_index)
            .map(|(_, rect)| *rect);
        let previous = row_index.checked_sub(1).and_then(|index| {
            self.rows
                .iter()
                .find(|(current, _)| *current == index)
                .map(|(_, rect)| *rect)
        });
        let reference = next.or(previous)?;
        let y = next.map_or(reference.bottom(), |rect| rect.top());
        Some(Rect::from_min_max(
            Pos2::new(reference.left(), y - height * 0.5),
            Pos2::new(reference.right(), y + height * 0.5),
        ))
    }
}

struct MergeSidePanelOutput {
    requested_result_scroll_y: Option<f32>,
    navigation_requested: bool,
    geometry: MergePanelGeometry,
    pending_line_action: Option<(MergeLineActionTarget, MergeLineAction)>,
}

struct MergeResultPanelOutput {
    scroll_y: f32,
    geometry: MergePanelGeometry,
}

#[derive(Clone, Copy, Debug)]
struct ConflictActionRects {
    take: Rect,
    drop: Rect,
}

#[derive(Clone, Copy, Debug)]
struct MergeSideDisplayRow<'a> {
    text: &'a str,
    reference_text: Option<&'a str>,
    line_number: Option<usize>,
    conflict_index: Option<usize>,
    side_resolved: bool,
    tone: MergeSideLineTone,
    show_conflict_actions: bool,
    action_target: Option<MergeLineActionTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedMergeSideDisplayRow {
    text: String,
    reference_text: Option<String>,
    line_number: Option<usize>,
    conflict_index: Option<usize>,
    side_resolved: bool,
    tone: MergeSideLineTone,
    show_conflict_actions: bool,
    action_target: Option<MergeLineActionTarget>,
}

#[derive(Clone, Copy, Debug)]
struct MergeResultDisplayRow<'a> {
    text: &'a str,
    reference_text: Option<&'a str>,
    conflict_index: Option<usize>,
    tone: MergeSideLineTone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BaseOnlyDisplayGroup {
    line_index: usize,
    line_count: usize,
    missing_side: MergeSide,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MergeEditSnapshot {
    document: MergeDocument,
    result_text: String,
    manual_result_lines: Vec<String>,
    manual_result_override: bool,
    local_conflict_cursor: usize,
    remote_conflict_cursor: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeCancelRequest {
    ExitNow,
    ShowConfirm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeSideDiffRow<'a> {
    Equal(&'a str),
    Deleted(&'a str),
    Added(&'a str),
    Replaced(&'a str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeChangeSide {
    Local,
    Remote,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MergeChange {
    base_start: usize,
    base_end: usize,
    side_start: usize,
    side_end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeBoundaryBias {
    Before,
    After,
}

pub fn parse_merge_args<I, S>(args: I) -> anyhow::Result<MergeArgs>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut items: Vec<String> = args.into_iter().map(Into::into).collect();
    if !items.is_empty() {
        items.remove(0);
    }

    let mut positional = Vec::new();
    let mut base = None;
    let mut local = None;
    let mut remote = None;
    let mut output = None;
    let mut repo_root = None;
    let mut stage = false;
    let mut theme = MergeTheme::Dark;
    let mut language = MergeLanguage::English;
    let mut iter = items.into_iter();

    while let Some(item) = iter.next() {
        if !item.starts_with("--") {
            positional.push(item);
            continue;
        }
        if item == "--stage" {
            stage = true;
            continue;
        }
        let Some(value) = iter.next() else {
            return Err(anyhow!("missing value for {item}"));
        };
        match item.as_str() {
            "--base" => base = Some(PathBuf::from(value)),
            "--local" => local = Some(PathBuf::from(value)),
            "--remote" | "--theirs" => remote = Some(PathBuf::from(value)),
            "--output" | "--merged" => output = Some(PathBuf::from(value)),
            "--repo-root" => repo_root = Some(PathBuf::from(value)),
            "--theme" => theme = parse_theme(&value)?,
            "--language" | "--lang" => language = parse_language(&value)?,
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }

    if positional.len() == 4 {
        base.get_or_insert_with(|| PathBuf::from(&positional[0]));
        local.get_or_insert_with(|| PathBuf::from(&positional[1]));
        remote.get_or_insert_with(|| PathBuf::from(&positional[2]));
        output.get_or_insert_with(|| PathBuf::from(&positional[3]));
    } else if !positional.is_empty() {
        return Err(anyhow!("expected 4 positional paths"));
    }

    Ok(MergeArgs {
        base: base.context("missing --base")?,
        local: local.context("missing --local")?,
        remote: remote.context("missing --remote")?,
        output: output.context("missing --output")?,
        repo_root,
        stage,
        theme,
        language,
    })
}

pub fn three_way_merge(base: &str, local: &str, remote: &str) -> MergeDocument {
    three_way_merge_with_options(base, local, remote, MergeIgnoreMode::None)
}

fn three_way_merge_with_options(
    base: &str,
    local: &str,
    remote: &str,
    ignore_mode: MergeIgnoreMode,
) -> MergeDocument {
    let base_lines = split_lines(base);
    let local_lines = split_lines(local);
    let remote_lines = split_lines(remote);
    if !merge_text_equal(base, remote, ignore_mode) && !base.is_empty() && local.is_empty() {
        return delete_modify_conflict_document(base_lines, local_lines, remote_lines);
    }
    if !merge_text_equal(base, local, ignore_mode) && !base.is_empty() && remote.is_empty() {
        return delete_modify_conflict_document(base_lines, local_lines, remote_lines);
    }
    merge_document_from_changes(&base_lines, &local_lines, &remote_lines, ignore_mode)
}

fn merge_document_from_changes(
    base_lines: &[String],
    local_lines: &[String],
    remote_lines: &[String],
    ignore_mode: MergeIgnoreMode,
) -> MergeDocument {
    let local_changes = diff_changes(base_lines, local_lines, ignore_mode);
    let remote_changes = diff_changes(base_lines, remote_lines, ignore_mode);
    let mut tagged_changes = local_changes
        .iter()
        .cloned()
        .map(|change| (MergeChangeSide::Local, change))
        .chain(
            remote_changes
                .iter()
                .cloned()
                .map(|change| (MergeChangeSide::Remote, change)),
        )
        .collect::<Vec<_>>();
    tagged_changes.sort_by_key(|(_, change)| (change.base_start, change.base_end));

    let mut lines = Vec::new();
    let mut conflicts = Vec::new();
    let mut base_cursor = 0;
    let mut change_index = 0;

    while change_index < tagged_changes.len() {
        let region_start = tagged_changes[change_index].1.base_start;
        push_resolved_lines(&mut lines, &base_lines[base_cursor..region_start]);

        let mut region_end = tagged_changes[change_index].1.base_end;
        let mut region_has_delete_only = tagged_changes[change_index].1.is_delete_only();
        change_index += 1;

        while change_index < tagged_changes.len() {
            let next = &tagged_changes[change_index].1;
            let overlaps = next.base_start < region_end
                || (next.base_start == region_end
                    && (region_start == region_end
                        || (region_has_delete_only && next.is_delete_only())));
            if !overlaps {
                break;
            }
            region_end = region_end.max(next.base_end);
            region_has_delete_only &= next.is_delete_only();
            change_index += 1;
        }

        push_merge_region(
            &mut lines,
            &mut conflicts,
            base_lines,
            local_lines,
            remote_lines,
            &local_changes,
            &remote_changes,
            region_start,
            region_end,
            ignore_mode,
        );
        base_cursor = region_end;
    }

    push_resolved_lines(&mut lines, &base_lines[base_cursor..]);

    MergeDocument { lines, conflicts }
}

#[allow(clippy::too_many_arguments)]
fn push_merge_region(
    lines: &mut Vec<MergeLine>,
    conflicts: &mut Vec<ConflictBlock>,
    base_lines: &[String],
    local_lines: &[String],
    remote_lines: &[String],
    local_changes: &[MergeChange],
    remote_changes: &[MergeChange],
    base_start: usize,
    base_end: usize,
    ignore_mode: MergeIgnoreMode,
) {
    let local_start =
        side_position_for_base_position(local_changes, base_start, MergeBoundaryBias::Before);
    let local_end = side_end_position_for_merge_region(local_changes, base_start, base_end);
    let remote_start =
        side_position_for_base_position(remote_changes, base_start, MergeBoundaryBias::Before);
    let remote_end = side_end_position_for_merge_region(remote_changes, base_start, base_end);
    let base_slice = &base_lines[base_start..base_end];
    let local_slice = &local_lines[local_start..local_end];
    let remote_slice = &remote_lines[remote_start..remote_end];

    if merge_lines_equal(local_slice, remote_slice, ignore_mode) {
        push_resolved_lines(lines, local_slice);
        return;
    }
    if merge_lines_equal(local_slice, base_slice, ignore_mode) {
        push_auto_resolved_side_region(lines, base_slice, remote_slice, MergeSide::Remote);
        return;
    }
    if merge_lines_equal(remote_slice, base_slice, ignore_mode) {
        push_auto_resolved_side_region(lines, base_slice, local_slice, MergeSide::Local);
        return;
    }

    push_conflict_region(lines, conflicts, base_slice, local_slice, remote_slice);
}

/// A zero-width insertion at a non-empty region's trailing boundary belongs to
/// the next merge region. Including it here turns an independent delete plus
/// insertion into a false conflict.
fn side_end_position_for_merge_region(
    changes: &[MergeChange],
    base_start: usize,
    base_end: usize,
) -> usize {
    let trailing_insertion = base_start < base_end
        && changes
            .iter()
            .any(|change| change.base_start == base_end && change.base_start == change.base_end);
    side_position_for_base_position(
        changes,
        base_end,
        if trailing_insertion {
            MergeBoundaryBias::Before
        } else {
            MergeBoundaryBias::After
        },
    )
}

fn push_resolved_lines(lines: &mut Vec<MergeLine>, result_lines: &[String]) {
    for result in result_lines {
        push_resolved_line(lines, result);
    }
}

fn push_resolved_line(lines: &mut Vec<MergeLine>, result: &str) {
    lines.push(MergeLine {
        base: Some(result.to_owned()),
        local: Some(result.to_owned()),
        remote: Some(result.to_owned()),
        result: result.to_owned(),
        include_in_result: true,
        kind: MergeLineKind::Resolved,
        conflict_index: None,
        local_resolved: true,
        remote_resolved: true,
        local_taken: false,
        remote_taken: false,
        base_only_resolved: false,
    });
}

fn push_auto_resolved_side_region(
    lines: &mut Vec<MergeLine>,
    base: &[String],
    side: &[String],
    changed_side: MergeSide,
) {
    for row in merge_diff_base_to_side(base, side) {
        match row {
            MergeSideDiffRow::Equal(text)
            | MergeSideDiffRow::Added(text)
            | MergeSideDiffRow::Replaced(text) => push_resolved_line(lines, text),
            MergeSideDiffRow::Deleted(text) => {
                push_base_only_display_line(lines, text, changed_side)
            }
        }
    }
}

fn push_base_only_display_line(lines: &mut Vec<MergeLine>, text: &str, changed_side: MergeSide) {
    let (local, remote) = match changed_side {
        MergeSide::Local => (None, Some(text.to_owned())),
        MergeSide::Remote => (Some(text.to_owned()), None),
    };
    lines.push(MergeLine {
        base: Some(text.to_owned()),
        local,
        remote,
        result: text.to_owned(),
        include_in_result: false,
        kind: MergeLineKind::Resolved,
        conflict_index: None,
        local_resolved: true,
        remote_resolved: true,
        local_taken: false,
        remote_taken: false,
        base_only_resolved: false,
    });
}

fn push_conflict_region(
    lines: &mut Vec<MergeLine>,
    conflicts: &mut Vec<ConflictBlock>,
    base: &[String],
    local: &[String],
    remote: &[String],
) {
    let conflict_index = conflicts.len();
    let max_len = base.len().max(local.len()).max(remote.len()).max(1);
    let mut line_indices = Vec::new();
    for index in 0..max_len {
        line_indices.push(lines.len());
        let base_line = base.get(index).cloned();
        lines.push(MergeLine {
            result: base_line.clone().unwrap_or_default(),
            include_in_result: false,
            base: base_line,
            local: local.get(index).cloned(),
            remote: remote.get(index).cloned(),
            kind: MergeLineKind::Conflict,
            conflict_index: Some(conflict_index),
            local_resolved: false,
            remote_resolved: false,
            local_taken: false,
            remote_taken: false,
            base_only_resolved: false,
        });
    }
    conflicts.push(ConflictBlock {
        index: conflict_index,
        base: base.to_vec(),
        local: local.to_vec(),
        remote: remote.to_vec(),
        line_indices,
    });
}

fn diff_changes(
    base: &[String],
    side: &[String],
    ignore_mode: MergeIgnoreMode,
) -> Vec<MergeChange> {
    let mut lcs = vec![vec![0; side.len() + 1]; base.len() + 1];
    for base_index in (0..base.len()).rev() {
        for side_index in (0..side.len()).rev() {
            lcs[base_index][side_index] =
                if merge_line_equal(&base[base_index], &side[side_index], ignore_mode) {
                    lcs[base_index + 1][side_index + 1] + 1
                } else {
                    lcs[base_index + 1][side_index].max(lcs[base_index][side_index + 1])
                };
        }
    }

    let mut changes = Vec::new();
    let mut base_index = 0;
    let mut side_index = 0;
    let mut pending_start = None;
    while base_index < base.len() && side_index < side.len() {
        if merge_line_equal(&base[base_index], &side[side_index], ignore_mode) {
            if let Some((base_start, side_start)) = pending_start.take() {
                changes.push(MergeChange {
                    base_start,
                    base_end: base_index,
                    side_start,
                    side_end: side_index,
                });
            }
            base_index += 1;
            side_index += 1;
        } else {
            pending_start.get_or_insert((base_index, side_index));
            if lcs[base_index + 1][side_index] > lcs[base_index][side_index + 1] {
                base_index += 1;
            } else {
                side_index += 1;
            }
        }
    }
    if base_index < base.len() || side_index < side.len() {
        pending_start.get_or_insert((base_index, side_index));
        base_index = base.len();
        side_index = side.len();
    }
    if let Some((base_start, side_start)) = pending_start.take() {
        changes.push(MergeChange {
            base_start,
            base_end: base_index,
            side_start,
            side_end: side_index,
        });
    }

    changes
}

fn merge_text_equal(left: &str, right: &str, ignore_mode: MergeIgnoreMode) -> bool {
    let left = split_lines(left);
    let right = split_lines(right);
    merge_lines_equal(&left, &right, ignore_mode)
}

fn merge_lines_equal(left: &[String], right: &[String], ignore_mode: MergeIgnoreMode) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| merge_line_equal(left, right, ignore_mode))
}

fn merge_line_equal(left: &str, right: &str, ignore_mode: MergeIgnoreMode) -> bool {
    match ignore_mode {
        MergeIgnoreMode::None => left == right,
        MergeIgnoreMode::TrimWhitespace => left.trim() == right.trim(),
        MergeIgnoreMode::IgnoreWhitespace => left
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .eq(right.chars().filter(|ch| !ch.is_whitespace())),
    }
}

fn side_position_for_base_position(
    changes: &[MergeChange],
    base_position: usize,
    bias: MergeBoundaryBias,
) -> usize {
    let mut base_cursor = 0;
    let mut side_cursor = 0;

    for change in changes {
        if base_position < change.base_start {
            return side_cursor + (base_position - base_cursor);
        }
        if base_position == change.base_start
            && change.base_start == change.base_end
            && bias == MergeBoundaryBias::Before
        {
            return side_cursor + (base_position - base_cursor);
        }
        if base_position == change.base_start && change.base_start < change.base_end {
            return side_cursor + (base_position - base_cursor);
        }

        if base_position < change.base_end {
            return match bias {
                MergeBoundaryBias::Before => change.side_start,
                MergeBoundaryBias::After => change.side_end,
            };
        }

        side_cursor = change.side_end;
        base_cursor = change.base_end;

        if base_position == change.base_end && bias == MergeBoundaryBias::Before {
            return side_cursor;
        }
    }

    side_cursor + (base_position - base_cursor)
}

impl MergeChange {
    fn is_delete_only(&self) -> bool {
        self.side_start == self.side_end && self.base_start < self.base_end
    }
}

fn delete_modify_conflict_document(
    base_lines: Vec<String>,
    local_lines: Vec<String>,
    remote_lines: Vec<String>,
) -> MergeDocument {
    let max_len = base_lines
        .len()
        .max(local_lines.len())
        .max(remote_lines.len());
    let line_indices = (0..max_len).collect::<Vec<_>>();
    let lines = (0..max_len)
        .map(|index| {
            let base = base_lines.get(index).cloned();
            let local = local_lines.get(index).cloned();
            let remote = remote_lines.get(index).cloned();
            MergeLine {
                result: base.clone().unwrap_or_default(),
                include_in_result: false,
                base,
                local,
                remote,
                kind: MergeLineKind::Conflict,
                conflict_index: Some(0),
                local_resolved: false,
                remote_resolved: false,
                local_taken: false,
                remote_taken: false,
                base_only_resolved: false,
            }
        })
        .collect();
    MergeDocument {
        lines,
        conflicts: vec![ConflictBlock {
            index: 0,
            base: base_lines,
            local: local_lines,
            remote: remote_lines,
            line_indices,
        }],
    }
}

impl MergeDocument {
    pub fn conflicts(&self) -> &[ConflictBlock] {
        &self.conflicts
    }

    pub fn result_text(&self) -> String {
        let mut text = self
            .lines
            .iter()
            .filter(|line| line.include_in_result)
            .flat_map(MergeLine::result_lines)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }

    fn apply_conflict(&mut self, index: usize, side: MergeSide) {
        self.accept_conflict_side_only(index, side);
    }

    pub fn accept_conflict_side_only(&mut self, index: usize, side: MergeSide) {
        let Some(conflict) = self.conflicts.get(index).cloned() else {
            return;
        };
        for line_index in conflict.line_indices {
            if let Some(line) = self.lines.get_mut(line_index) {
                match side {
                    MergeSide::Local => {
                        line.set_side(MergeSide::Local, true);
                        line.set_side(MergeSide::Remote, false);
                    }
                    MergeSide::Remote => {
                        line.set_side(MergeSide::Remote, true);
                        line.set_side(MergeSide::Local, false);
                    }
                }
            }
        }
    }

    pub fn take_conflict_side(&mut self, index: usize, side: MergeSide) {
        self.set_conflict_side(index, side, MergeLineAction::Take);
    }

    pub fn drop_conflict_side(&mut self, index: usize, side: MergeSide) {
        self.set_conflict_side(index, side, MergeLineAction::Drop);
    }

    fn take_base_only_group(&mut self, line_index: usize, side: MergeSide) {
        self.set_base_only_group(line_index, side, MergeLineAction::Take);
    }

    fn drop_base_only_group(&mut self, line_index: usize, side: MergeSide) {
        self.set_base_only_group(line_index, side, MergeLineAction::Drop);
    }

    pub fn unresolved_conflict_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|conflict| {
                self.conflict_side_unresolved(conflict.index, MergeSide::Local)
                    || self.conflict_side_unresolved(conflict.index, MergeSide::Remote)
            })
            .count()
    }

    pub fn unresolved_conflict_count_for_side(&self, side: MergeSide) -> usize {
        self.conflicts
            .iter()
            .filter(|conflict| self.conflict_side_unresolved(conflict.index, side))
            .count()
    }

    fn conflict_fully_resolved(&self, index: usize) -> bool {
        !self.conflict_side_unresolved(index, MergeSide::Local)
            && !self.conflict_side_unresolved(index, MergeSide::Remote)
    }

    pub fn conflict_side_resolved(&self, index: usize, side: MergeSide) -> bool {
        !self.conflict_side_unresolved(index, side)
    }

    fn set_conflict_side(&mut self, index: usize, side: MergeSide, action: MergeLineAction) {
        let Some(conflict) = self.conflicts.get(index).cloned() else {
            return;
        };
        for line_index in conflict.line_indices {
            if let Some(line) = self.lines.get_mut(line_index) {
                line.set_side(side, action == MergeLineAction::Take);
            }
        }
    }

    fn set_base_only_group(&mut self, line_index: usize, side: MergeSide, action: MergeLineAction) {
        if !self.line_is_base_only_missing_side(line_index, side) {
            return;
        }

        let mut start = line_index;
        while start > 0 && self.line_is_base_only_missing_side(start - 1, side) {
            start -= 1;
        }

        let mut end = line_index;
        while end + 1 < self.lines.len() && self.line_is_base_only_missing_side(end + 1, side) {
            end += 1;
        }

        let include_base = action == MergeLineAction::Drop;
        for line in &mut self.lines[start..=end] {
            line.include_in_result = include_base;
            line.base_only_resolved = true;
        }
    }

    fn line_is_base_only_missing_side(&self, line_index: usize, side: MergeSide) -> bool {
        self.lines
            .get(line_index)
            .and_then(MergeLine::base_only_missing_side)
            == Some(side)
    }

    fn conflict_side_unresolved(&self, index: usize, side: MergeSide) -> bool {
        let Some(conflict) = self.conflicts.get(index) else {
            return false;
        };
        conflict.line_indices.iter().any(|line_index| {
            self.lines
                .get(*line_index)
                .is_some_and(|line| !line.side_resolved(side))
        })
    }
}

impl MergeLine {
    fn is_base_only_display(&self) -> bool {
        self.kind == MergeLineKind::Resolved
            && !self.include_in_result
            && !self.base_only_resolved
            && self.base_only_missing_side_raw().is_some()
    }

    fn base_only_missing_side(&self) -> Option<MergeSide> {
        if !self.is_base_only_display() {
            return None;
        }
        self.base_only_missing_side_raw()
    }

    fn base_only_missing_side_raw(&self) -> Option<MergeSide> {
        self.base.as_ref()?;
        match (self.local.is_none(), self.remote.is_none()) {
            (true, false) => Some(MergeSide::Local),
            (false, true) => Some(MergeSide::Remote),
            _ => None,
        }
    }

    fn result_lines(&self) -> Vec<&str> {
        let mut lines = Vec::new();
        if self.local_taken {
            if let Some(local) = &self.local {
                lines.push(local.as_str());
            }
        }
        if self.remote_taken {
            if let Some(remote) = &self.remote {
                lines.push(remote.as_str());
            }
        }
        if self.local_taken || self.remote_taken {
            return lines;
        }
        if lines.is_empty() && self.include_in_result {
            lines.push(self.result.as_str());
        }
        lines
    }

    fn side_resolved(&self, side: MergeSide) -> bool {
        match side {
            MergeSide::Local => self.local_resolved,
            MergeSide::Remote => self.remote_resolved,
        }
    }

    fn set_side(&mut self, side: MergeSide, take: bool) {
        match side {
            MergeSide::Local => {
                self.local_resolved = true;
                self.local_taken = take;
            }
            MergeSide::Remote => {
                self.remote_resolved = true;
                self.remote_taken = take;
            }
        }
        self.kind = if self.local_resolved && self.remote_resolved {
            MergeLineKind::Resolved
        } else {
            MergeLineKind::Conflict
        };
        self.include_in_result = if self.conflict_index.is_some() {
            self.local_taken || self.remote_taken
        } else {
            self.kind != MergeLineKind::Conflict
        };
    }
}

pub struct MergeToolApp {
    args: MergeArgs,
    sources: Option<MergeSourceText>,
    initial_document: MergeDocument,
    document: MergeDocument,
    result_text: String,
    manual_result_lines: Vec<String>,
    local_display_rows: Vec<CachedMergeSideDisplayRow>,
    remote_display_rows: Vec<CachedMergeSideDisplayRow>,
    local_scroll_anchors: Vec<(f32, f32)>,
    remote_scroll_anchors: Vec<(f32, f32)>,
    manual_result_override: bool,
    shared_scroll_y: f32,
    local_conflict_cursor: usize,
    remote_conflict_cursor: usize,
    theme: MergeTheme,
    language: MergeLanguage,
    status: Option<String>,
    write_task: Option<Receiver<anyhow::Result<()>>>,
    undo_stack: Vec<MergeEditSnapshot>,
    redo_stack: Vec<MergeEditSnapshot>,
    display_epoch: u64,
    show_cancel_confirm: bool,
    connector_debug: MergeConnectorDebug,
    ignore_mode: MergeIgnoreMode,
    highlight_mode: MergeHighlightMode,
    collapse_unchanged: bool,
}

impl MergeToolApp {
    pub fn from_args(args: MergeArgs) -> anyhow::Result<Self> {
        let base = read_text(&args.base)?;
        let local = read_text(&args.local)?;
        let remote = read_text(&args.remote)?;
        let document = three_way_merge(&base, &local, &remote);
        let mut app = Self::new(args, document);
        app.sources = Some(MergeSourceText {
            base,
            local,
            remote,
        });
        Ok(app)
    }

    pub fn new(args: MergeArgs, document: MergeDocument) -> Self {
        let result_text = document.result_text();
        let manual_result_lines = merge_result_display_rows(&document)
            .into_iter()
            .map(|row| row.text.to_owned())
            .collect();
        let local_display_rows = cached_merge_side_display_rows(&document, MergeSide::Local);
        let remote_display_rows = cached_merge_side_display_rows(&document, MergeSide::Remote);
        let result_display_rows = merge_result_display_rows(&document);
        let local_scroll_anchors = merge_cached_scroll_anchors(
            &document,
            MergeSide::Local,
            &result_display_rows,
            &local_display_rows,
        );
        let remote_scroll_anchors = merge_cached_scroll_anchors(
            &document,
            MergeSide::Remote,
            &result_display_rows,
            &remote_display_rows,
        );
        let initial_document = document.clone();
        Self {
            theme: args.theme,
            language: args.language,
            args,
            sources: None,
            initial_document,
            document,
            result_text,
            manual_result_lines,
            local_display_rows,
            remote_display_rows,
            local_scroll_anchors,
            remote_scroll_anchors,
            manual_result_override: false,
            shared_scroll_y: 0.0,
            local_conflict_cursor: 0,
            remote_conflict_cursor: 0,
            status: None,
            write_task: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            display_epoch: 0,
            show_cancel_confirm: false,
            connector_debug: merge_connector_debug_mode(),
            ignore_mode: MergeIgnoreMode::None,
            highlight_mode: MergeHighlightMode::Lines,
            collapse_unchanged: false,
        }
    }

    pub fn run_from_env() -> eframe::Result<()> {
        let args = match parse_merge_args(env::args()) {
            Ok(args) => args,
            Err(error) => {
                eprintln!(
                    "Usage: git-agent-merge --base <base> --local <local> --remote <remote> --output <merged> [--repo-root <repo> --stage] [--theme dark|light] [--language en|zh]\n{error}"
                );
                std::process::exit(2);
            }
        };
        let title = format!("Git Agent Merge - {}", args.output.display());
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_icon(merge_app_icon_data())
                .with_decorations(false)
                .with_transparent(true)
                .with_inner_size([1180.0, 760.0])
                .with_min_inner_size([980.0, 620.0]),
            ..Default::default()
        };
        eframe::run_native(
            &title,
            options,
            Box::new(move |cc| {
                prefer_rounded_merge_window_corners(cc);
                crate::theme::install(&cc.egui_ctx);
                // The standalone merge binary has its own egui context. Register SVG loaders here
                // as well, otherwise toolbar icons render as the missing-image warning glyph.
                egui_extras::install_image_loaders(&cc.egui_ctx);
                apply_merge_theme(&cc.egui_ctx, args.theme);
                let app = Self::from_args(args).unwrap_or_else(|error| {
                    let fallback = MergeArgs {
                        base: PathBuf::new(),
                        local: PathBuf::new(),
                        remote: PathBuf::new(),
                        output: PathBuf::new(),
                        repo_root: None,
                        stage: false,
                        theme: MergeTheme::Dark,
                        language: MergeLanguage::English,
                    };
                    let mut app = Self::new(fallback, three_way_merge("", "", ""));
                    app.status = Some(error.to_string());
                    app
                });
                Ok(Box::new(app))
            }),
        )
    }

    fn snapshot(&self) -> MergeEditSnapshot {
        MergeEditSnapshot {
            document: self.document.clone(),
            result_text: self.result_text.clone(),
            manual_result_lines: self.manual_result_lines.clone(),
            manual_result_override: self.manual_result_override,
            local_conflict_cursor: self.local_conflict_cursor,
            remote_conflict_cursor: self.remote_conflict_cursor,
        }
    }

    fn restore_snapshot(&mut self, snapshot: MergeEditSnapshot) {
        self.document = snapshot.document;
        self.result_text = snapshot.result_text;
        self.manual_result_lines = snapshot.manual_result_lines;
        self.manual_result_override = snapshot.manual_result_override;
        self.local_conflict_cursor = snapshot.local_conflict_cursor;
        self.remote_conflict_cursor = snapshot.remote_conflict_cursor;
        self.rebuild_display_rows();
    }

    fn reset_from_sources(&mut self, ignore_mode: MergeIgnoreMode) {
        let Some(sources) = self.sources.as_ref() else {
            return;
        };
        self.ignore_mode = ignore_mode;
        self.document = three_way_merge_with_options(
            &sources.base,
            &sources.local,
            &sources.remote,
            ignore_mode,
        );
        self.initial_document = self.document.clone();
        self.result_text = self.document.result_text();
        self.manual_result_lines = merge_result_display_rows(&self.document)
            .into_iter()
            .map(|row| row.text.to_owned())
            .collect();
        self.manual_result_override = false;
        self.shared_scroll_y = 0.0;
        self.local_conflict_cursor = 0;
        self.remote_conflict_cursor = 0;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.rebuild_display_rows();
    }

    fn finish_document_edit(&mut self, before: MergeEditSnapshot) {
        self.rebuild_display_rows();
        if self.snapshot() != before {
            self.undo_stack.push(before);
            self.redo_stack.clear();
        }
    }

    fn has_unsaved_edits(&self) -> bool {
        self.document != self.initial_document
            || self.result_text != self.initial_document.result_text()
    }

    fn unresolved_conflict_count(&self) -> usize {
        if self.manual_result_override {
            0
        } else {
            self.document.unresolved_conflict_count()
        }
    }

    fn unresolved_conflict_count_for_side(&self, side: MergeSide) -> usize {
        if self.manual_result_override {
            0
        } else {
            self.document.unresolved_conflict_count_for_side(side)
        }
    }

    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn can_apply_result(&self) -> bool {
        self.write_task.is_none() && self.unresolved_conflict_count() == 0
    }

    fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.redo_stack.push(current);
        self.restore_snapshot(previous);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.undo_stack.push(current);
        self.restore_snapshot(next);
        true
    }

    fn request_cancel(&mut self) -> MergeCancelRequest {
        if self.has_unsaved_edits() {
            self.show_cancel_confirm = true;
            MergeCancelRequest::ShowConfirm
        } else {
            MergeCancelRequest::ExitNow
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let (undo_requested, redo_requested) = ctx.input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            (
                ctrl && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                ctrl && i.key_pressed(egui::Key::Y)
                    || (ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z)),
            )
        });
        if undo_requested && self.can_undo() {
            self.undo();
        } else if redo_requested && self.can_redo() {
            self.redo();
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.has_unsaved_edits() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_cancel_confirm = true;
        }
    }

    fn accept_conflict(&mut self, side: MergeSide) {
        if self.manual_result_override {
            return;
        }
        let before = self.snapshot();
        let index = match side {
            MergeSide::Local => self.local_conflict_cursor,
            MergeSide::Remote => self.remote_conflict_cursor,
        };
        self.document.apply_conflict(index, side);
        self.result_text = self.document.result_text();
        self.reset_manual_result_lines();
        let conflict_count = self.document.conflicts().len();
        if conflict_count > 0 {
            self.local_conflict_cursor = (self.local_conflict_cursor + 1).min(conflict_count - 1);
            self.remote_conflict_cursor = (self.remote_conflict_cursor + 1).min(conflict_count - 1);
        }
        self.finish_document_edit(before);
    }

    fn apply_line_action(
        &mut self,
        target: MergeLineActionTarget,
        side: MergeSide,
        action: MergeLineAction,
    ) {
        if self.manual_result_override {
            return;
        }
        let before = self.snapshot();
        match (target, action) {
            (MergeLineActionTarget::Conflict(index), MergeLineAction::Take) => {
                self.document.take_conflict_side(index, side)
            }
            (MergeLineActionTarget::Conflict(index), MergeLineAction::Drop) => {
                self.document.drop_conflict_side(index, side)
            }
            (MergeLineActionTarget::BaseOnlyGroup(line_index), MergeLineAction::Take) => {
                self.document.take_base_only_group(line_index, side)
            }
            (MergeLineActionTarget::BaseOnlyGroup(line_index), MergeLineAction::Drop) => {
                self.document.drop_base_only_group(line_index, side)
            }
        }
        self.result_text = self.document.result_text();
        self.reset_manual_result_lines();
        self.finish_document_edit(before);
    }

    fn reset_manual_result_lines(&mut self) {
        if self.manual_result_override {
            return;
        }
        self.manual_result_lines = merge_result_display_rows(&self.document)
            .into_iter()
            .map(|row| row.text.to_owned())
            .collect();
        self.rebuild_display_rows();
    }

    fn rebuild_display_rows(&mut self) {
        self.local_display_rows = cached_merge_side_display_rows(&self.document, MergeSide::Local);
        self.remote_display_rows =
            cached_merge_side_display_rows(&self.document, MergeSide::Remote);
        let result_display_rows = merge_result_display_rows(&self.document);
        self.local_scroll_anchors = merge_cached_scroll_anchors(
            &self.document,
            MergeSide::Local,
            &result_display_rows,
            &self.local_display_rows,
        );
        self.remote_scroll_anchors = merge_cached_scroll_anchors(
            &self.document,
            MergeSide::Remote,
            &result_display_rows,
            &self.remote_display_rows,
        );
        // Rows may appear or disappear after accepting a deletion or undoing it. Give all
        // three ScrollAreas a fresh identity so egui cannot retain geometry from another shape.
        self.display_epoch = self.display_epoch.wrapping_add(1);
    }

    fn uses_virtual_merge_rows(&self) -> bool {
        self.manual_result_lines.len() >= MERGE_VIRTUAL_ROW_THRESHOLD
            || self.local_display_rows.len() >= MERGE_VIRTUAL_ROW_THRESHOLD
            || self.remote_display_rows.len() >= MERGE_VIRTUAL_ROW_THRESHOLD
    }

    fn cached_scroll_anchors(&self, side: MergeSide) -> &[(f32, f32)] {
        match side {
            MergeSide::Local => &self.local_scroll_anchors,
            MergeSide::Remote => &self.remote_scroll_anchors,
        }
    }

    fn cached_side_scroll_y_for_result_scroll(&self, side: MergeSide, result_scroll_y: f32) -> f32 {
        let result_row = result_scroll_y / MERGE_CODE_ROW_HEIGHT;
        merge_mapped_scroll_row(self.cached_scroll_anchors(side), result_row, true)
            * MERGE_CODE_ROW_HEIGHT
    }

    fn cached_result_scroll_y_for_side_scroll(&self, side: MergeSide, side_scroll_y: f32) -> f32 {
        let side_row = side_scroll_y / MERGE_CODE_ROW_HEIGHT;
        merge_mapped_scroll_row(self.cached_scroll_anchors(side), side_row, false)
            * MERGE_CODE_ROW_HEIGHT
    }

    fn finish_manual_result_edit(&mut self, before: MergeEditSnapshot) {
        self.manual_result_override = true;
        self.result_text = self.manual_result_lines.join("\n");
        if !self.result_text.is_empty() {
            self.result_text.push('\n');
        }
        self.finish_document_edit(before);
    }

    fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.theme = match self.theme {
            MergeTheme::Dark => MergeTheme::Light,
            MergeTheme::Light => MergeTheme::Dark,
        };
        apply_merge_theme(ctx, self.theme);
    }

    fn toggle_language(&mut self) {
        self.language = match self.language {
            MergeLanguage::English => MergeLanguage::Chinese,
            MergeLanguage::Chinese => MergeLanguage::English,
        };
    }

    fn write_output(&mut self) {
        if self.write_task.is_some() {
            return;
        }
        if self.unresolved_conflict_count() > 0 {
            self.status = Some(mt(self.language, "resolve_all_conflicts").to_owned());
            return;
        }
        let args = self.args.clone();
        let result_text = self.result_text.clone();
        let (sender, receiver) = mpsc::channel();
        self.write_task = Some(receiver);
        self.status = Some(mt(self.language, "applying").to_owned());
        thread::spawn(move || {
            let _ = sender.send(write_merge_output(&args, &result_text));
        });
    }

    fn poll_write_task(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.write_task.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(())) => std::process::exit(0),
            Ok(Err(error)) => {
                self.status = Some(format!(
                    "{} {}: {error}",
                    mt(self.language, "write_failed"),
                    self.args.output.display()
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.write_task = Some(receiver);
                ctx.request_repaint_after(Duration::from_millis(60));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = Some(mt(self.language, "write_stopped").to_owned());
            }
        }
    }
}

fn merge_app_icon_data() -> egui::IconData {
    const SIZE: usize = 64;
    let mut rgba = vec![0_u8; SIZE * SIZE * 4];
    let green = [21, 196, 151, 255];
    let blue = [47, 111, 234, 255];

    // Same visual language as the main app: transparent canvas, crisp Git routes,
    // hollow nodes. A green change is inserted into the blue result line.
    paint_merge_icon_line(&mut rgba, 16, 39, 48, 39, blue);
    paint_merge_icon_line(&mut rgba, 24, 17, 24, 34, green);
    paint_merge_icon_line(&mut rgba, 18, 28, 24, 34, green);
    paint_merge_icon_line(&mut rgba, 30, 28, 24, 34, green);
    paint_merge_icon_ring(&mut rgba, 24, 17, green);
    paint_merge_icon_ring(&mut rgba, 48, 39, blue);

    egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

fn paint_merge_icon_ring(rgba: &mut [u8], cx: usize, cy: usize, color: [u8; 4]) {
    const RADIUS: usize = 8;
    const WIDTH: usize = 4;
    let outer_sq = (RADIUS * RADIUS) as isize;
    let inner_sq = ((RADIUS - WIDTH) * (RADIUS - WIDTH)) as isize;
    for y in cy - RADIUS..=cy + RADIUS {
        for x in cx - RADIUS..=cx + RADIUS {
            let dx = x as isize - cx as isize;
            let dy = y as isize - cy as isize;
            let distance_sq = dx * dx + dy * dy;
            if distance_sq <= outer_sq && distance_sq >= inner_sq {
                paint_merge_icon_pixel(rgba, x, y, color);
            }
        }
    }
}

fn paint_merge_icon_line(
    rgba: &mut [u8],
    start_x: usize,
    start_y: usize,
    end_x: usize,
    end_y: usize,
    color: [u8; 4],
) {
    let steps = start_x.abs_diff(end_x).max(start_y.abs_diff(end_y)).max(1);
    for step in 0..=steps {
        let progress = step as f32 / steps as f32;
        let x = (start_x as f32 + (end_x as f32 - start_x as f32) * progress).round() as usize;
        let y = (start_y as f32 + (end_y as f32 - start_y as f32) * progress).round() as usize;
        paint_merge_icon_disc(rgba, x, y, color);
    }
}

fn paint_merge_icon_disc(rgba: &mut [u8], cx: usize, cy: usize, color: [u8; 4]) {
    const SIZE: usize = 64;
    const RADIUS: usize = 4;
    let radius_sq = (RADIUS * RADIUS) as isize;
    for y in cy.saturating_sub(RADIUS)..=(cy + RADIUS).min(SIZE - 1) {
        for x in cx.saturating_sub(RADIUS)..=(cx + RADIUS).min(SIZE - 1) {
            let dx = x as isize - cx as isize;
            let dy = y as isize - cy as isize;
            if dx * dx + dy * dy <= radius_sq {
                paint_merge_icon_pixel(rgba, x, y, color);
            }
        }
    }
}

fn paint_merge_icon_pixel(rgba: &mut [u8], x: usize, y: usize, color: [u8; 4]) {
    const SIZE: usize = 64;
    let index = (y * SIZE + x) * 4;
    rgba[index..index + 4].copy_from_slice(&color);
}

pub fn write_merge_output(args: &MergeArgs, result_text: &str) -> anyhow::Result<()> {
    fs::write(&args.output, result_text)
        .with_context(|| format!("failed to write {}", args.output.display()))?;
    if args.stage {
        let repo_root = args
            .repo_root
            .as_deref()
            .context("missing --repo-root for --stage")?;
        stage_merge_output(repo_root, &args.output)?;
    }
    Ok(())
}

fn stage_merge_output(repo_root: &Path, output: &Path) -> anyhow::Result<()> {
    let path_arg = output.strip_prefix(repo_root).unwrap_or(output);
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("add")
        .arg("--")
        .arg(path_arg)
        .status()
        .with_context(|| format!("failed to stage {}", output.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("git add failed for {}", output.display()))
    }
}

impl App for MergeToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_write_task(ctx);
        self.handle_keyboard_shortcuts(ctx);
        self.handle_close_request(ctx);
        let palette = merge_palette(self.theme);
        apply_merge_theme(ctx, self.theme);

        egui::TopBottomPanel::top("merge_window_titlebar")
            .exact_height(MERGE_WINDOW_TITLEBAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| merge_custom_title_bar(ui, ctx, self, palette));

        egui::TopBottomPanel::top("merge_toolbar")
            .exact_height(38.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel)
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| merge_toolbar(ui, self, palette));

        egui::TopBottomPanel::bottom("merge_footer")
            .exact_height(56.0)
            .frame(egui::Frame::new().fill(palette.panel))
            .show(ctx, |ui| merge_footer(ui, self, palette));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.bg))
            .show(ctx, |ui| merge_editor_columns(ui, self, palette));

        merge_cancel_confirm_dialog(ctx, self, palette);
    }
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(ToOwned::to_owned).collect()
}

fn read_text(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn parse_theme(value: &str) -> anyhow::Result<MergeTheme> {
    match value.to_ascii_lowercase().as_str() {
        "dark" | "night" => Ok(MergeTheme::Dark),
        "light" | "day" => Ok(MergeTheme::Light),
        _ => Err(anyhow!("unknown theme {value}")),
    }
}

fn parse_language(value: &str) -> anyhow::Result<MergeLanguage> {
    match value.to_ascii_lowercase().as_str() {
        "en" | "english" => Ok(MergeLanguage::English),
        "zh" | "cn" | "chinese" => Ok(MergeLanguage::Chinese),
        _ => Err(anyhow!("unknown language {value}")),
    }
}

const MERGE_COLUMN_GAP: f32 = 12.0;
const MERGE_OVERVIEW_WIDTH: f32 = 22.0;
const MERGE_OVERVIEW_GAP: f32 = 6.0;
const MERGE_OVERVIEW_COLUMN_WIDTH: f32 = 4.0;
const MERGE_WINDOW_TITLEBAR_HEIGHT: f32 = 32.0;

fn merge_custom_title_bar(
    ui: &mut Ui,
    ctx: &egui::Context,
    app: &MergeToolApp,
    palette: MergePalette,
) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), palette.panel);

    let controls_width = 112.0;
    let drag_rect = Rect::from_min_max(
        rect.left_top(),
        Pos2::new(rect.right() - controls_width, rect.bottom()),
    );
    let drag_response = ui.interact(
        drag_rect,
        ui.id().with("merge_window_title_drag"),
        Sense::click_and_drag(),
    );
    if drag_response.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if drag_response.double_clicked() {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }

    let controls_rect = Rect::from_min_max(
        Pos2::new(rect.right() - controls_width, rect.top() + 4.0),
        Pos2::new(rect.right() - 7.0, rect.bottom() - 4.0),
    );
    let title_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 10.0, rect.top()),
        Pos2::new(controls_rect.left() - 8.0, rect.bottom()),
    );
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_rect), |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Git Agent Merge")
                    .strong()
                    .color(palette.text),
            );
            if let Some(name) = app.args.output.file_name().and_then(|name| name.to_str()) {
                ui.label(RichText::new(format!("- {name}")).color(palette.muted));
            }
        });
    });
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(controls_rect), |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if merge_window_control_button(ui, "×", true, palette).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            if merge_window_control_button(ui, if maximized { "❐" } else { "□" }, false, palette)
                .clicked()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if merge_window_control_button(ui, "−", false, palette).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        });
    });
}

fn merge_window_control_button(
    ui: &mut Ui,
    label: &str,
    close: bool,
    palette: MergePalette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(34.0, 22.0), Sense::click());
    let fill = if close {
        if response.hovered() {
            Color32::from_rgb(192, 55, 43)
        } else {
            Color32::TRANSPARENT
        }
    } else if response.hovered() {
        palette.panel_soft
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(16.0),
        if close && response.hovered() {
            Color32::WHITE
        } else {
            palette.text
        },
    );
    response
}

#[cfg(target_os = "windows")]
fn prefer_rounded_merge_window_corners(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;
    let Ok(window_handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return;
    };
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            handle.hwnd.get() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const u32 as _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn prefer_rounded_merge_window_corners(_: &eframe::CreationContext<'_>) {}

fn merge_toolbar(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    let unresolved_conflicts = app.unresolved_conflict_count();
    let row_rect = ui.max_rect().shrink2(Vec2::new(
        8.0,
        (ui.max_rect().height() - 28.0).max(0.0) * 0.5,
    ));
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().interact_size.y = 24.0;
            ui.label(
                RichText::new(mt(app.language, "title"))
                    .strong()
                    .color(palette.text),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    unresolved_conflicts,
                    mt(app.language, "conflicts")
                ))
                .monospace()
                .color(if unresolved_conflicts > 0 {
                    palette.conflict_text
                } else {
                    palette.muted
                }),
            );
            ui.add_space(10.0);
            ui.label(RichText::new(mt(app.language, "auto_applied")).color(palette.muted));
            ui.add_space(16.0);
            let controls_width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(controls_width, ui.available_height()),
                Layout::right_to_left(Align::Center),
                |ui| {
                    if merge_toolbar_toggle_button(
                        ui,
                        MergeToolbarToggleIcon::Language,
                        merge_language_label(app.language),
                        false,
                        palette,
                    )
                    .clicked()
                    {
                        app.toggle_language();
                    }
                    if merge_toolbar_toggle_button(
                        ui,
                        match app.theme {
                            MergeTheme::Dark => MergeToolbarToggleIcon::Moon,
                            MergeTheme::Light => MergeToolbarToggleIcon::Sun,
                        },
                        merge_theme_label(app.language, app.theme),
                        false,
                        palette,
                    )
                    .clicked()
                    {
                        app.toggle_theme(ui.ctx());
                    }
                    let collapse_label = if app.collapse_unchanged {
                        mt(app.language, "expand_unchanged")
                    } else {
                        mt(app.language, "collapse_unchanged")
                    };
                    if merge_toolbar_toggle_button(
                        ui,
                        if app.collapse_unchanged {
                            MergeToolbarToggleIcon::Expand
                        } else {
                            MergeToolbarToggleIcon::Collapse
                        },
                        collapse_label,
                        false,
                        palette,
                    )
                    .clicked()
                    {
                        app.collapse_unchanged = !app.collapse_unchanged;
                        // The displayed document becomes shorter or longer. Do not carry an offset from
                        // the previous shape into the new ScrollAreas.
                        app.shared_scroll_y = 0.0;
                        app.rebuild_display_rows();
                    }
                    merge_highlight_mode_combo(ui, app, palette);
                    merge_ignore_mode_combo(ui, app, palette);
                    ui.label(
                        RichText::new(format!(
                            "{} {} {}",
                            mt(app.language, "no_changes"),
                            unresolved_conflicts,
                            mt(app.language, "conflict_count")
                        ))
                        .color(palette.muted),
                    );
                    if let Some(status) = &app.status {
                        ui.label(RichText::new(status).color(palette.conflict_text));
                    }
                },
            );
        },
    );
}

fn merge_toolbar_toggle_button(
    ui: &mut Ui,
    icon: MergeToolbarToggleIcon,
    tooltip: &str,
    active: bool,
    palette: MergePalette,
) -> egui::Response {
    let icon_color = if active {
        Color32::WHITE
    } else {
        palette.muted
    };
    let image = egui::Image::new(merge_toolbar_icon_source(icon))
        .fit_to_exact_size(Vec2::splat(15.0))
        .tint(icon_color);
    let response = ui.add(
        egui::Button::image(image)
            .min_size(Vec2::splat(28.0))
            .fill(if active {
                palette.accent
            } else {
                Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(4)),
    );
    response.on_hover_text(tooltip)
}

fn merge_toolbar_icon_source(icon: MergeToolbarToggleIcon) -> egui::ImageSource<'static> {
    match icon {
        MergeToolbarToggleIcon::Collapse => {
            egui::include_image!("../assets/icons/merge-collapse.svg")
        }
        MergeToolbarToggleIcon::Expand => egui::include_image!("../assets/icons/merge-expand.svg"),
        MergeToolbarToggleIcon::Sun => egui::include_image!("../assets/icons/merge-sun.svg"),
        MergeToolbarToggleIcon::Moon => egui::include_image!("../assets/icons/merge-moon.svg"),
        MergeToolbarToggleIcon::Language => {
            egui::include_image!("../assets/icons/merge-language.svg")
        }
    }
}

fn merge_highlight_mode_combo(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    ComboBox::from_id_salt("merge_highlight_mode")
        .width(124.0)
        .selected_text(
            RichText::new(merge_highlight_mode_label(app.language, app.highlight_mode))
                .size(11.0)
                .color(palette.text),
        )
        .show_ui(ui, |ui| {
            for mode in [MergeHighlightMode::Lines, MergeHighlightMode::Words] {
                ui.selectable_value(
                    &mut app.highlight_mode,
                    mode,
                    merge_highlight_mode_label(app.language, mode),
                );
            }
        });
}

fn merge_ignore_mode_combo(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    let mut selected = app.ignore_mode;
    ComboBox::from_id_salt("merge_ignore_mode")
        .width(132.0)
        .selected_text(
            RichText::new(merge_ignore_mode_label(app.language, selected))
                .size(11.0)
                .color(palette.text),
        )
        .show_ui(ui, |ui| {
            for mode in [
                MergeIgnoreMode::None,
                MergeIgnoreMode::TrimWhitespace,
                MergeIgnoreMode::IgnoreWhitespace,
            ] {
                ui.selectable_value(
                    &mut selected,
                    mode,
                    merge_ignore_mode_label(app.language, mode),
                );
            }
        });
    if selected != app.ignore_mode {
        app.reset_from_sources(selected);
    }
}

fn merge_footer(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    let writing = app.write_task.is_some();
    let can_apply = app.can_apply_result();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        if ui
            .add_enabled(
                !app.manual_result_override,
                egui::Button::new(mt(app.language, "accept_left")),
            )
            .clicked()
        {
            app.accept_conflict(MergeSide::Local);
        }
        if ui
            .add_enabled(
                !app.manual_result_override,
                egui::Button::new(mt(app.language, "accept_right")),
            )
            .clicked()
        {
            app.accept_conflict(MergeSide::Remote);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if ui
                .add_enabled(
                    !writing,
                    egui::Button::new(mt(app.language, "cancel")).min_size(Vec2::new(88.0, 30.0)),
                )
                .clicked()
                && app.request_cancel() == MergeCancelRequest::ExitNow
            {
                std::process::exit(MERGE_TOOL_CANCEL_EXIT_CODE);
            }
            let apply_label = if writing {
                mt(app.language, "applying")
            } else {
                mt(app.language, "apply")
            };
            if ui
                .add_enabled(
                    can_apply,
                    egui::Button::new(RichText::new(apply_label).strong().color(Color32::WHITE))
                        .min_size(Vec2::new(88.0, 30.0))
                        .fill(palette.accent),
                )
                .clicked()
            {
                app.write_output();
            }
        });
    });
}

fn merge_cancel_confirm_dialog(ctx: &egui::Context, app: &mut MergeToolApp, palette: MergePalette) {
    if !app.show_cancel_confirm {
        return;
    }

    let mut open = true;
    let mut discard = false;
    let mut continue_merge = false;
    egui::Window::new(mt(app.language, "cancel_merge_title"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, dialog::top_anchor_offset())
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.add_space(4.0);
            ui.label(RichText::new(mt(app.language, "cancel_merge_message")).color(palette.text));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(mt(app.language, "cancel_merge_continue"))
                        .clicked()
                    {
                        continue_merge = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(mt(app.language, "cancel_merge_discard"))
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(palette.accent),
                        )
                        .clicked()
                    {
                        discard = true;
                    }
                });
            });
        });

    if discard {
        std::process::exit(MERGE_TOOL_CANCEL_EXIT_CODE);
    }
    app.show_cancel_confirm = open && !continue_merge;
}

fn merge_editor_columns(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    let rect = ui
        .available_rect_before_wrap()
        .shrink2(Vec2::new(10.0, 8.0));
    let gap = MERGE_COLUMN_GAP;
    let panes_width = rect.width() - MERGE_OVERVIEW_WIDTH - MERGE_OVERVIEW_GAP;
    let left_w = (panes_width * 0.32).max(250.0);
    let result_w = (panes_width * 0.34).max(280.0);
    let right_w = (panes_width - left_w - result_w - gap * 2.0).max(250.0);
    let left = Rect::from_min_size(rect.min, Vec2::new(left_w, rect.height()));
    let result = Rect::from_min_size(
        Pos2::new(left.right() + gap, rect.top()),
        Vec2::new(result_w, rect.height()),
    );
    let right = Rect::from_min_size(
        Pos2::new(result.right() + gap, rect.top()),
        Vec2::new(right_w, rect.height()),
    );
    let overview = Rect::from_min_size(
        Pos2::new(right.right() + MERGE_OVERVIEW_GAP, rect.top()),
        Vec2::new(MERGE_OVERVIEW_WIDTH, rect.height()),
    );

    let requested_scroll_y = app.shared_scroll_y;
    let mut result_output = MergeResultPanelOutput {
        scroll_y: requested_scroll_y,
        geometry: MergePanelGeometry::default(),
    };
    let use_virtual_rows = app.uses_virtual_merge_rows();
    // Collapsed tails replace many rows with one marker. Their display coordinates are no longer
    // the source-document coordinates used by the normal three-way anchor mapper.
    let use_direct_shared_scroll = app.collapse_unchanged;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(result), |ui| {
        result_output =
            merge_result_panel(ui, app, "merge_result_scroll", requested_scroll_y, palette);
    });
    let frame_scroll_y = result_output.scroll_y;
    let mut next_shared_scroll_y = frame_scroll_y;
    let local_scroll_y = if use_direct_shared_scroll {
        frame_scroll_y
    } else if use_virtual_rows {
        app.cached_side_scroll_y_for_result_scroll(MergeSide::Local, frame_scroll_y)
    } else {
        merge_side_scroll_y_for_result_scroll(&app.document, MergeSide::Local, frame_scroll_y)
    };
    let local_output = ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(left), |ui| {
            merge_side_panel(
                ui,
                app,
                MergeSide::Local,
                "merge_local_scroll",
                local_scroll_y,
                palette,
            )
        })
        .inner;
    if let Some(scroll_y) = local_output.requested_result_scroll_y {
        if !app.collapse_unchanged || local_output.navigation_requested {
            next_shared_scroll_y = scroll_y;
        }
    }
    let remote_scroll_y = if use_direct_shared_scroll {
        frame_scroll_y
    } else if use_virtual_rows {
        app.cached_side_scroll_y_for_result_scroll(MergeSide::Remote, frame_scroll_y)
    } else {
        merge_side_scroll_y_for_result_scroll(&app.document, MergeSide::Remote, frame_scroll_y)
    };
    let remote_output = ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(right), |ui| {
            merge_side_panel(
                ui,
                app,
                MergeSide::Remote,
                "merge_remote_scroll",
                remote_scroll_y,
                palette,
            )
        })
        .inner;
    if let Some(scroll_y) = remote_output.requested_result_scroll_y {
        if !app.collapse_unchanged || remote_output.navigation_requested {
            next_shared_scroll_y = scroll_y;
        }
    }
    // Virtual ScrollAreas only record geometry for rows materialized in this frame.
    // The connector painter already ignores blocks without recorded geometry, so keep it
    // enabled for large files and paint only the currently visible conflict fragments.
    paint_merge_block_connectors(
        ui,
        &app.document,
        &local_output.geometry,
        &result_output.geometry,
        &remote_output.geometry,
        app.connector_debug,
        palette,
    );
    if let Some(scroll_y) = merge_overview_target(
        ui,
        overview,
        app,
        next_shared_scroll_y,
        &result_output.geometry,
        palette,
    ) {
        next_shared_scroll_y = scroll_y;
        ui.ctx().request_repaint();
    }
    app.shared_scroll_y = next_shared_scroll_y;
    // Apply after every pane and connector used the same document snapshot. Mutating while the
    // local pane is rendering used to leave result/remote geometry from a different state.
    if let Some((target, side, action)) = local_output
        .pending_line_action
        .map(|(target, action)| (target, MergeSide::Local, action))
        .or_else(|| {
            remote_output
                .pending_line_action
                .map(|(target, action)| (target, MergeSide::Remote, action))
        })
    {
        app.apply_line_action(target, side, action);
        ui.ctx().request_repaint();
    }
}

fn merge_side_panel(
    ui: &mut Ui,
    app: &mut MergeToolApp,
    side: MergeSide,
    scroll_id: &'static str,
    scroll_y: f32,
    palette: MergePalette,
) -> MergeSidePanelOutput {
    let mut requested_result_scroll_y = None;
    let mut navigation_requested = false;
    let mut geometry = MergePanelGeometry::default();
    let mut pending_line_action = None;
    let path = match side {
        MergeSide::Local => app.args.local.clone(),
        MergeSide::Remote => app.args.remote.clone(),
    };
    let title = match side {
        MergeSide::Local => mt(app.language, "local"),
        MergeSide::Remote => mt(app.language, "remote"),
    };
    merge_panel_frame(ui, palette, |ui| {
        side_header(ui, title, &path, palette);
        let nav_target = side_conflict_nav(ui, app, side, palette);
        ui.add_space(8.0);
        let use_virtual_rows = app.uses_virtual_merge_rows();
        let rows = match side {
            MergeSide::Local => &app.local_display_rows,
            MergeSide::Remote => &app.remote_display_rows,
        };
        let visible_rows = merge_visible_tail_len(rows, app.collapse_unchanged);
        let hidden_rows = rows.len().saturating_sub(visible_rows);
        let display_rows = visible_rows + usize::from(hidden_rows > 0);
        let output = ScrollArea::vertical()
            .id_salt((scroll_id, app.display_epoch))
            .vertical_scroll_offset(scroll_y)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .show_rows(ui, MERGE_CODE_ROW_HEIGHT, display_rows, |ui, row_range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(ui.available_width());
                let cursor = match side {
                    MergeSide::Local => app.local_conflict_cursor,
                    MergeSide::Remote => app.remote_conflict_cursor,
                };
                for display_index in row_range.clone() {
                    if display_index >= visible_rows {
                        merge_collapsed_result_row(ui, hidden_rows, app.language, palette);
                        continue;
                    }
                    let row = &rows[display_index];
                    let background = merge_side_background_run(
                        rows,
                        display_index,
                        row_range.start,
                        row_range.end.min(visible_rows),
                        cursor,
                        app.highlight_mode,
                        palette,
                    );
                    let rect = merge_code_row(
                        ui,
                        row.line_number,
                        side,
                        &row.text,
                        row.reference_text.as_deref(),
                        row.conflict_index,
                        row.side_resolved,
                        row.tone,
                        row.show_conflict_actions && !app.manual_result_override,
                        row.action_target,
                        background,
                        app.highlight_mode,
                        palette,
                        &mut pending_line_action,
                    );
                    geometry.record_row(display_index, rect);
                }
            });
        // Missing-side markers use the same visible-row geometry as the virtual list. Keeping
        // them enabled is required for large files; off-screen groups naturally return None.
        if !app.manual_result_override {
            paint_base_only_side_overlays(
                ui,
                &app.document,
                side,
                &geometry,
                palette,
                &mut pending_line_action,
            );
        }
        if let Some(conflict_index) = nav_target {
            navigation_requested = true;
            requested_result_scroll_y =
                merge_result_scroll_y_for_conflict(&app.document, conflict_index);
            ui.ctx().request_repaint();
        } else {
            let clamped_scroll_y = merge_clamp_scroll_offset(
                output.state.offset.y,
                output.content_size.y,
                output.inner_rect.height(),
            );
            if (clamped_scroll_y - scroll_y).abs() > f32::EPSILON {
                requested_result_scroll_y = Some(if app.collapse_unchanged {
                    clamped_scroll_y
                } else if use_virtual_rows {
                    app.cached_result_scroll_y_for_side_scroll(side, clamped_scroll_y)
                } else {
                    merge_result_scroll_y_for_side_scroll(&app.document, side, clamped_scroll_y)
                });
                ui.ctx().request_repaint();
            }
        }
    });
    MergeSidePanelOutput {
        requested_result_scroll_y,
        navigation_requested,
        geometry,
        pending_line_action,
    }
}

fn merge_result_panel(
    ui: &mut Ui,
    app: &mut MergeToolApp,
    scroll_id: &'static str,
    scroll_y: f32,
    palette: MergePalette,
) -> MergeResultPanelOutput {
    let mut next_scroll_y = scroll_y;
    let mut geometry = MergePanelGeometry::default();
    merge_panel_frame(ui, palette, |ui| {
        result_header(ui, app, palette);
        merge_result_nav_spacer(ui);
        ui.add_space(8.0);
        if app.manual_result_lines.is_empty() {
            app.manual_result_lines
                .push(mt(app.language, "result_placeholder").to_owned());
        }
        let mut changed_lines = Vec::new();
        let line_count = app.manual_result_lines.len();
        let result_display_rows =
            (!app.manual_result_override).then(|| merge_result_display_rows(&app.document));
        let result_row_styles = if app.manual_result_override {
            vec![(MergeSideLineTone::Unchanged, false); line_count]
        } else {
            result_display_rows
                .as_ref()
                .expect("result rows exist outside manual edit mode")
                .into_iter()
                .map(|row| {
                    let active = row.conflict_index.is_some_and(|conflict_index| {
                        (conflict_index == app.local_conflict_cursor
                            || conflict_index == app.remote_conflict_cursor)
                            && !app.document.conflict_fully_resolved(conflict_index)
                    });
                    (row.tone, active)
                })
                .collect::<Vec<_>>()
        };
        let visible_rows = merge_visible_result_len(&result_row_styles, app.collapse_unchanged);
        let hidden_rows = line_count.saturating_sub(visible_rows);
        let display_rows = visible_rows + usize::from(hidden_rows > 0);
        let output = ScrollArea::vertical()
            .id_salt((scroll_id, app.display_epoch))
            .vertical_scroll_offset(scroll_y)
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .show_rows(ui, MERGE_CODE_ROW_HEIGHT, display_rows, |ui, row_range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(ui.available_width());
                for result_index in row_range.clone() {
                    if result_index >= visible_rows {
                        merge_collapsed_result_row(ui, hidden_rows, app.language, palette);
                        continue;
                    }
                    let (tone, _) = result_row_styles
                        .get(result_index)
                        .copied()
                        .unwrap_or((MergeSideLineTone::Unchanged, false));
                    let background = merge_result_background_run(
                        &result_row_styles,
                        result_index,
                        row_range.start,
                        row_range.end,
                        app.highlight_mode,
                        palette,
                    );
                    let reference_text = result_display_rows
                        .as_ref()
                        .and_then(|rows| rows.get(result_index))
                        .and_then(|row| row.reference_text);
                    let (rect, before_line) = merge_editable_result_row(
                        ui,
                        result_index,
                        &mut app.manual_result_lines[result_index],
                        tone,
                        background,
                        app.highlight_mode,
                        reference_text,
                        palette,
                    );
                    geometry.record_row(result_index, rect);
                    if let Some(before_line) = before_line {
                        changed_lines.push((result_index, before_line));
                    }
                }
            });
        if !changed_lines.is_empty() {
            let mut before = app.snapshot();
            for (index, line) in changed_lines {
                before.manual_result_lines[index] = line;
            }
            app.finish_manual_result_edit(before);
        }
        next_scroll_y = merge_clamp_scroll_offset(
            output.state.offset.y,
            output.content_size.y,
            output.inner_rect.height(),
        );
    });
    MergeResultPanelOutput {
        scroll_y: next_scroll_y,
        geometry,
    }
}

fn merge_visible_tail_len(rows: &[CachedMergeSideDisplayRow], collapse: bool) -> usize {
    if !collapse {
        return rows.len();
    }
    let last_changed = rows
        .iter()
        .rposition(|row| row.conflict_index.is_some() || row.tone != MergeSideLineTone::Unchanged)
        .map(|index| index + 1)
        .unwrap_or(0);
    let keep = (last_changed + MERGE_COLLAPSE_CONTEXT_ROWS).min(rows.len());
    (rows.len().saturating_sub(keep) >= MERGE_COLLAPSE_MIN_UNCHANGED_ROWS)
        .then_some(keep)
        .unwrap_or(rows.len())
}

fn merge_clamp_scroll_offset(offset_y: f32, content_height: f32, viewport_height: f32) -> f32 {
    offset_y.clamp(0.0, (content_height - viewport_height).max(0.0))
}

fn merge_visible_result_len(styles: &[(MergeSideLineTone, bool)], collapse: bool) -> usize {
    if !collapse {
        return styles.len();
    }
    let last_changed = styles
        .iter()
        .rposition(|(tone, active)| *active || *tone != MergeSideLineTone::Unchanged)
        .map(|index| index + 1)
        .unwrap_or(0);
    let keep = (last_changed + MERGE_COLLAPSE_CONTEXT_ROWS).min(styles.len());
    (styles.len().saturating_sub(keep) >= MERGE_COLLAPSE_MIN_UNCHANGED_ROWS)
        .then_some(keep)
        .unwrap_or(styles.len())
}

fn merge_collapsed_result_row(
    ui: &mut Ui,
    hidden_rows: usize,
    language: MergeLanguage,
    palette: MergePalette,
) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), MERGE_CODE_ROW_HEIGHT),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(2),
        color_with_opacity(palette.panel_soft, 0.7),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("… {hidden_rows} {} …", mt(language, "unchanged_lines")),
        FontId::monospace(MERGE_CODE_FONT_SIZE),
        palette.muted,
    );
}

fn merge_overview_target(
    ui: &mut Ui,
    overview: Rect,
    app: &MergeToolApp,
    scroll_y: f32,
    result_geometry: &MergePanelGeometry,
    palette: MergePalette,
) -> Option<f32> {
    // The overview must mirror displayed rows, not source rows. In collapsed mode each hidden
    // tail becomes one marker row, otherwise the map advertises a scroll range the editors do not
    // have and scrolling eventually snaps back to the first row.
    let columns = [
        merge_visible_side_overview_tones(&app.local_display_rows, app.collapse_unchanged),
        merge_visible_result_overview_tones(app),
        merge_visible_side_overview_tones(&app.remote_display_rows, app.collapse_unchanged),
    ];
    let max_rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    if max_rows == 0 || overview.height() < 16.0 {
        return None;
    }
    // The map never grows taller than the actual code viewport.  A short document should get a
    // short map, while a long document keeps a map as tall as the visible code area.
    let result_rows = columns[1].len().max(1);
    let content_height = result_rows as f32 * MERGE_CODE_ROW_HEIGHT;
    let visible_height = merge_overview_visible_height(result_geometry)
        .unwrap_or_else(|| overview.height())
        .min(overview.height())
        .max(MERGE_CODE_ROW_HEIGHT);
    let map_height = content_height.min(visible_height).max(8.0);
    let track = Rect::from_min_size(
        Pos2::new(overview.left() + 1.0, overview.top()),
        Vec2::new((overview.width() - 2.0).max(1.0), map_height),
    );
    let viewport_height = visible_height.min(content_height);
    let max_scroll = (content_height - viewport_height).max(0.0);
    let viewport_ratio = (viewport_height / content_height).clamp(0.04, 1.0);
    let start_ratio = if max_scroll > 0.0 {
        (scroll_y / max_scroll).clamp(0.0, 1.0 - viewport_ratio)
    } else {
        0.0
    };
    let viewport = Rect::from_min_size(
        Pos2::new(
            track.left(),
            egui::lerp(track.top()..=track.bottom(), start_ratio),
        ),
        Vec2::new(track.width(), (track.height() * viewport_ratio).max(3.0)),
    );
    ui.painter().rect_filled(
        viewport.intersect(track),
        egui::CornerRadius::same(2),
        color_with_opacity(palette.accent, 0.54),
    );
    for (column_index, tones) in columns.iter().enumerate() {
        let total_rows = tones.len();
        let mut run_start = 0;
        while run_start < total_rows {
            let tone = tones[run_start];
            let mut run_end = run_start + 1;
            while run_end < total_rows && tones[run_end] == tone {
                run_end += 1;
            }
            let Some(color) = merge_overview_tone_color(tone, palette) else {
                run_start = run_end;
                continue;
            };
            let left = track.left() + column_index as f32 * (MERGE_OVERVIEW_COLUMN_WIDTH + 2.0);
            let top = egui::remap(
                run_start as f32,
                0.0..=total_rows.max(1) as f32,
                track.top()..=track.bottom(),
            );
            let bottom = egui::remap(
                run_end as f32,
                0.0..=total_rows.max(1) as f32,
                track.top()..=track.bottom(),
            )
            .max(top + 1.0);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(left, top),
                    Pos2::new(left + MERGE_OVERVIEW_COLUMN_WIDTH, bottom),
                ),
                0.0,
                color,
            );
            run_start = run_end;
        }
    }
    let response = ui.interact(
        track.expand2(Vec2::new(3.0, 0.0)),
        ui.make_persistent_id(("merge_overview", max_rows, overview.left() as i32)),
        Sense::click_and_drag(),
    );
    if !response.clicked() && !response.dragged() {
        return None;
    }
    let pointer = response.interact_pointer_pos()?;
    let ratio = ((pointer.y - track.top()) / track.height()).clamp(0.0, 1.0);
    let clicked_column = (0..columns.len())
        .min_by(|left, right| {
            let center = |index: usize| {
                track.left()
                    + index as f32 * (MERGE_OVERVIEW_COLUMN_WIDTH + 2.0)
                    + MERGE_OVERVIEW_COLUMN_WIDTH * 0.5
            };
            (pointer.x - center(*left))
                .abs()
                .total_cmp(&(pointer.x - center(*right)).abs())
        })
        .unwrap_or(1);
    let source_row = ratio * columns[clicked_column].len().max(1) as f32;
    let result_row = if app.collapse_unchanged {
        ratio * result_rows as f32
    } else {
        match clicked_column {
            0 => merge_mapped_scroll_row(
                app.cached_scroll_anchors(MergeSide::Local),
                source_row,
                false,
            ),
            2 => merge_mapped_scroll_row(
                app.cached_scroll_anchors(MergeSide::Remote),
                source_row,
                false,
            ),
            _ => source_row,
        }
    };
    Some((result_row * MERGE_CODE_ROW_HEIGHT - viewport_height * 0.5).clamp(0.0, max_scroll))
}

fn merge_overview_visible_height(geometry: &MergePanelGeometry) -> Option<f32> {
    let first = geometry.rows.first()?.1;
    let last = geometry.rows.last()?.1;
    Some((last.bottom() - first.top()).max(MERGE_CODE_ROW_HEIGHT))
}

fn merge_result_overview_tones(app: &MergeToolApp) -> Vec<MergeSideLineTone> {
    if app.manual_result_override {
        return vec![MergeSideLineTone::Unchanged; app.manual_result_lines.len()];
    }
    merge_result_display_rows(&app.document)
        .into_iter()
        .map(|row| row.tone)
        .collect()
}

fn merge_visible_side_overview_tones(
    rows: &[CachedMergeSideDisplayRow],
    collapse: bool,
) -> Vec<MergeSideLineTone> {
    let visible_rows = merge_visible_tail_len(rows, collapse);
    let mut tones = rows
        .iter()
        .take(visible_rows)
        .map(|row| row.tone)
        .collect::<Vec<_>>();
    if visible_rows < rows.len() {
        // The editor draws one collapsed-tail marker after the retained context.
        tones.push(MergeSideLineTone::Unchanged);
    }
    tones
}

fn merge_visible_result_overview_tones(app: &MergeToolApp) -> Vec<MergeSideLineTone> {
    let result_tones = merge_result_overview_tones(app);
    let visible_rows = merge_visible_result_len(
        &result_tones
            .iter()
            .copied()
            .map(|tone| (tone, false))
            .collect::<Vec<_>>(),
        app.collapse_unchanged,
    );
    let mut tones = result_tones
        .iter()
        .copied()
        .take(visible_rows)
        .collect::<Vec<_>>();
    if visible_rows < result_tones.len() {
        tones.push(MergeSideLineTone::Unchanged);
    }
    tones
}

fn merge_overview_tone_color(tone: MergeSideLineTone, palette: MergePalette) -> Option<Color32> {
    match tone {
        MergeSideLineTone::Added => Some(palette.added_text),
        MergeSideLineTone::BaseOnly => Some(palette.base_only_text),
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced => Some(palette.conflict_text),
        MergeSideLineTone::Unchanged => None,
    }
}

fn result_header(ui: &mut Ui, app: &mut MergeToolApp, palette: MergePalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let title_w = 118.0;
    ui.painter().text(
        Pos2::new(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        mt(app.language, "result"),
        FontId::proportional(13.0),
        palette.text,
    );
    let path_rect = Rect::from_min_max(
        Pos2::new(rect.left() + title_w, rect.top()),
        rect.right_bottom(),
    );
    ui.painter().with_clip_rect(path_rect).text(
        path_rect.left_center(),
        Align2::LEFT_CENTER,
        app.args.output.display().to_string(),
        FontId::monospace(12.0),
        palette.muted,
    );
}

fn merge_result_nav_spacer(ui: &mut Ui) {
    ui.allocate_exact_size(
        Vec2::new(ui.available_width(), MERGE_NAV_BUTTON_SIZE),
        Sense::hover(),
    );
}

fn side_header(ui: &mut Ui, title: &str, path: &Path, palette: MergePalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let title_w = 118.0;
    ui.painter().text(
        Pos2::new(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        palette.text,
    );
    let path_rect = Rect::from_min_max(
        Pos2::new(rect.left() + title_w, rect.top()),
        rect.right_top() + Vec2::new(0.0, rect.height()),
    );
    ui.painter().with_clip_rect(path_rect).text(
        path_rect.left_center(),
        Align2::LEFT_CENTER,
        path.display().to_string(),
        FontId::monospace(12.0),
        palette.muted,
    );
}

fn side_conflict_nav(
    ui: &mut Ui,
    app: &mut MergeToolApp,
    side: MergeSide,
    palette: MergePalette,
) -> Option<usize> {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(width, MERGE_NAV_BUTTON_SIZE),
        Layout::left_to_right(Align::Center),
        |ui| {
            let conflict_count = app.unresolved_conflict_count_for_side(side);
            let mut cursor = match side {
                MergeSide::Local => app.local_conflict_cursor,
                MergeSide::Remote => app.remote_conflict_cursor,
            };
            let enabled = conflict_count > 0;
            let mut navigation_requested = false;
            if nav_icon_button(ui, enabled, NavDirection::Previous, palette) {
                navigation_requested = true;
                previous_unresolved_conflict(&app.document, side, &mut cursor);
            }
            if nav_icon_button(ui, enabled, NavDirection::Next, palette) {
                navigation_requested = true;
                next_unresolved_conflict(&app.document, side, &mut cursor);
            }
            match side {
                MergeSide::Local => app.local_conflict_cursor = cursor,
                MergeSide::Remote => app.remote_conflict_cursor = cursor,
            }
            let position = if app.manual_result_override {
                0
            } else {
                unresolved_position(&app.document, side, cursor)
            };
            ui.label(
                RichText::new(format!("{} / {}", position, conflict_count)).color(palette.muted),
            );
            navigation_requested.then_some(cursor)
        },
    )
    .inner
}

fn nav_icon_button(
    ui: &mut Ui,
    enabled: bool,
    direction: NavDirection,
    palette: MergePalette,
) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(MERGE_NAV_BUTTON_SIZE), sense);
    let fill = if !enabled {
        ui.visuals().widgets.noninteractive.bg_fill
    } else if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    let color = if enabled {
        palette.muted
    } else {
        palette.muted.gamma_multiply(0.45)
    };

    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(3), fill);
    paint_nav_chevron(ui, rect, direction, color);
    enabled && response.clicked()
}

fn paint_nav_chevron(ui: &mut Ui, rect: Rect, direction: NavDirection, color: Color32) {
    let center = rect.center();
    let half_width = 4.5;
    let half_height = 2.8;
    let stroke = egui::Stroke::new(1.5, color);
    let (left, middle, right) = match direction {
        NavDirection::Previous => (
            Pos2::new(center.x - half_width, center.y + half_height),
            Pos2::new(center.x, center.y - half_height),
            Pos2::new(center.x + half_width, center.y + half_height),
        ),
        NavDirection::Next => (
            Pos2::new(center.x - half_width, center.y - half_height),
            Pos2::new(center.x, center.y + half_height),
            Pos2::new(center.x + half_width, center.y - half_height),
        ),
    };
    ui.painter().line_segment([left, middle], stroke);
    ui.painter().line_segment([middle, right], stroke);
}

fn merge_code_row(
    ui: &mut Ui,
    line_number: Option<usize>,
    side: MergeSide,
    text: &str,
    reference_text: Option<&str>,
    conflict_index: Option<usize>,
    side_resolved: bool,
    tone: MergeSideLineTone,
    show_conflict_actions: bool,
    action_target: Option<MergeLineActionTarget>,
    background: Option<(Color32, usize)>,
    highlight_mode: MergeHighlightMode,
    palette: MergePalette,
    pending_action: &mut Option<(MergeLineActionTarget, MergeLineAction)>,
) -> Rect {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), MERGE_CODE_ROW_HEIGHT),
        Sense::hover(),
    );
    if let Some((fill, row_count)) = background {
        let fill_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(
                rect.right(),
                rect.bottom() + MERGE_CODE_ROW_HEIGHT * row_count.saturating_sub(1) as f32,
            ),
        );
        ui.painter()
            .rect_filled(fill_rect, egui::CornerRadius::ZERO, fill);
    }
    let can_show_actions = show_conflict_actions
        && action_target.is_some()
        && match action_target {
            Some(MergeLineActionTarget::Conflict(_)) => !side_resolved,
            Some(MergeLineActionTarget::BaseOnlyGroup(_)) => true,
            None => false,
        };
    if can_show_actions {
        let action_target = action_target.expect("checked action target");
        let action_rects = conflict_action_rects(rect, side);
        let drop_response = ui.put(action_rects.drop, egui::Button::new("X"));
        if drop_response.clicked() {
            *pending_action = Some((action_target, MergeLineAction::Drop));
        }
        let arrow = match side {
            MergeSide::Local => ">>",
            MergeSide::Remote => "<<",
        };
        let take_response = ui.put(action_rects.take, egui::Button::new(arrow));
        if take_response.clicked() {
            *pending_action = Some((action_target, MergeLineAction::Take));
        }
    }
    if let Some(line_number) = line_number {
        ui.painter().text(
            Pos2::new(rect.left() + 58.0, rect.center().y),
            Align2::LEFT_CENTER,
            format!("{line_number:>4}"),
            FontId::monospace(MERGE_CODE_FONT_SIZE),
            palette.muted,
        );
    }
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 100.0, rect.top()),
        rect.right_bottom(),
    );
    let text_color = match tone {
        MergeSideLineTone::Added => palette.added_text,
        MergeSideLineTone::BaseOnly => palette.base_only_text,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced
            if conflict_index.is_some() && !side_resolved =>
        {
            palette.conflict_text
        }
        _ => palette.text,
    };
    if highlight_mode == MergeHighlightMode::Words
        && tone != MergeSideLineTone::Unchanged
        && !text.is_empty()
    {
        paint_word_highlight_text(
            ui,
            text_rect,
            text,
            reference_text.unwrap_or(""),
            text_color,
        );
    } else {
        ui.painter().with_clip_rect(text_rect).text(
            text_rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::monospace(MERGE_CODE_FONT_SIZE),
            text_color,
        );
    }
    rect
}

fn paint_word_highlight_text(
    ui: &Ui,
    text_rect: Rect,
    text: &str,
    reference: &str,
    text_color: Color32,
) {
    let font = FontId::monospace(MERGE_CODE_FONT_SIZE);
    paint_word_highlight_backgrounds(ui, text_rect, text, reference, text_color);
    ui.painter().with_clip_rect(text_rect).text(
        text_rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        font,
        text_color,
    );
}

fn paint_word_highlight_backgrounds(
    ui: &Ui,
    text_rect: Rect,
    text: &str,
    reference: &str,
    text_color: Color32,
) {
    let font = FontId::monospace(MERGE_CODE_FONT_SIZE);
    for range in merge_word_highlight_ranges(text, reference) {
        let prefix_width = merge_text_width(ui, &text[..range.start], &font, text_color);
        let token_width = merge_text_width(ui, &text[range.clone()], &font, text_color);
        let highlight = Rect::from_min_size(
            Pos2::new(text_rect.left() + prefix_width, text_rect.top() + 2.0),
            Vec2::new(token_width.max(2.0), text_rect.height() - 4.0),
        );
        ui.painter().rect_filled(
            highlight,
            egui::CornerRadius::same(2),
            color_with_opacity(text_color, 0.26),
        );
    }
}

fn merge_text_width(ui: &Ui, text: &str, font: &FontId, color: Color32) -> f32 {
    ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font.clone(), color)
            .rect
            .width()
    })
}

fn merge_code_row_fill(
    tone: MergeSideLineTone,
    unresolved_conflict: bool,
    active_conflict: bool,
    highlight_mode: MergeHighlightMode,
    palette: MergePalette,
) -> Option<Color32> {
    // Word mode still needs a quiet line-level cue for every changed block. The stronger
    // token highlights alone made non-conflicting insertions read as unchanged whitespace.
    let show_word_mode_change = highlight_mode == MergeHighlightMode::Words;
    let fill = match tone {
        // An addition inside an unresolved conflict is still a competing choice, not a
        // settled addition. Using the near-white green insertion fill here made the whole
        // block disappear in word mode while replacement conflicts stayed visibly peach.
        MergeSideLineTone::Added if unresolved_conflict => {
            if active_conflict {
                palette.active_conflict_fill
            } else {
                palette.conflict_fill
            }
        }
        MergeSideLineTone::Added if show_word_mode_change => palette.added_fill,
        MergeSideLineTone::BaseOnly => palette.base_only_fill,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced
            if unresolved_conflict || show_word_mode_change =>
        {
            if active_conflict {
                palette.active_conflict_fill
            } else {
                palette.conflict_fill
            }
        }
        _ => return None,
    };

    Some(merge_highlight_fill(
        tone,
        fill,
        active_conflict,
        highlight_mode,
    ))
}

fn merge_side_row_fill(
    row: &CachedMergeSideDisplayRow,
    cursor: usize,
    highlight_mode: MergeHighlightMode,
    palette: MergePalette,
) -> Option<Color32> {
    merge_code_row_fill(
        row.tone,
        row.conflict_index.is_some() && !row.side_resolved,
        row.conflict_index == Some(cursor) && !row.side_resolved,
        highlight_mode,
        palette,
    )
}

fn merge_side_background_run(
    rows: &[CachedMergeSideDisplayRow],
    index: usize,
    visible_start: usize,
    visible_end: usize,
    cursor: usize,
    highlight_mode: MergeHighlightMode,
    palette: MergePalette,
) -> Option<(Color32, usize)> {
    let row = rows.get(index)?;
    let fill = merge_side_row_fill(row, cursor, highlight_mode, palette)?;
    let same_run = |other: &CachedMergeSideDisplayRow| {
        other.conflict_index == row.conflict_index
            && merge_side_row_fill(other, cursor, highlight_mode, palette) == Some(fill)
    };
    let is_start = index == visible_start
        || rows
            .get(index.saturating_sub(1))
            .is_none_or(|previous| !same_run(previous));
    if !is_start {
        return None;
    }

    let mut count = 1;
    while index + count < visible_end && rows.get(index + count).is_some_and(same_run) {
        count += 1;
    }
    Some((fill, count))
}

fn merge_word_highlight_ranges(text: &str, reference: &str) -> Vec<Range<usize>> {
    let text_tokens = merge_word_tokens(text);
    if text_tokens.is_empty() {
        return Vec::new();
    }
    let reference_tokens = merge_word_tokens(reference);
    if reference_tokens.is_empty() {
        return text_tokens;
    }

    const MAX_EXACT_TOKENS: usize = 128;
    if text_tokens.len() > MAX_EXACT_TOKENS || reference_tokens.len() > MAX_EXACT_TOKENS {
        return merge_word_fallback_ranges(text, reference, text_tokens);
    }

    let mut lcs = vec![vec![0_usize; reference_tokens.len() + 1]; text_tokens.len() + 1];
    for text_index in (0..text_tokens.len()).rev() {
        for reference_index in (0..reference_tokens.len()).rev() {
            lcs[text_index][reference_index] = if text[text_tokens[text_index].clone()]
                == reference[reference_tokens[reference_index].clone()]
            {
                lcs[text_index + 1][reference_index + 1] + 1
            } else {
                lcs[text_index + 1][reference_index].max(lcs[text_index][reference_index + 1])
            };
        }
    }

    let mut changed = Vec::new();
    let (mut text_index, mut reference_index) = (0, 0);
    while text_index < text_tokens.len() && reference_index < reference_tokens.len() {
        if text[text_tokens[text_index].clone()]
            == reference[reference_tokens[reference_index].clone()]
        {
            text_index += 1;
            reference_index += 1;
        } else if lcs[text_index + 1][reference_index] >= lcs[text_index][reference_index + 1] {
            changed.push(text_tokens[text_index].clone());
            text_index += 1;
        } else {
            reference_index += 1;
        }
    }
    changed.extend(text_tokens.into_iter().skip(text_index));
    changed
}

fn merge_word_tokens(text: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = token_start.take() {
                tokens.push(start..index);
            }
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }
    if let Some(start) = token_start {
        tokens.push(start..text.len());
    }
    tokens
}

fn merge_word_fallback_ranges(
    text: &str,
    reference: &str,
    text_tokens: Vec<Range<usize>>,
) -> Vec<Range<usize>> {
    let text_chars = text.chars().collect::<Vec<_>>();
    let reference_chars = reference.chars().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < text_chars.len()
        && prefix < reference_chars.len()
        && text_chars[prefix] == reference_chars[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < text_chars.len().saturating_sub(prefix)
        && suffix < reference_chars.len().saturating_sub(prefix)
        && text_chars[text_chars.len() - 1 - suffix]
            == reference_chars[reference_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let changed_start = text_chars[..prefix]
        .iter()
        .map(|character| character.len_utf8())
        .sum::<usize>();
    let changed_end = text.len()
        - text_chars[text_chars.len().saturating_sub(suffix)..]
            .iter()
            .map(|character| character.len_utf8())
            .sum::<usize>();
    text_tokens
        .into_iter()
        .filter(|token| token.start < changed_end && token.end > changed_start)
        .collect()
}

fn paint_base_only_gap_marker_rect(ui: &Ui, marker_rect: Rect, palette: MergePalette) {
    ui.painter().rect_filled(
        marker_rect,
        egui::CornerRadius::same(1),
        color_with_opacity(palette.base_only_fill, 0.9),
    );
}

fn base_only_gap_marker_rect(row_rect: Rect) -> Rect {
    let center_y = row_rect.center().y;
    let half_height = MERGE_BASE_ONLY_MARKER_HEIGHT * 0.5;
    Rect::from_min_max(
        Pos2::new(row_rect.left() + 58.0, center_y - half_height),
        Pos2::new(row_rect.right() - 8.0, center_y + half_height),
    )
}

fn paint_base_only_side_overlays(
    ui: &mut Ui,
    document: &MergeDocument,
    side: MergeSide,
    geometry: &MergePanelGeometry,
    palette: MergePalette,
    pending_action: &mut Option<(MergeLineActionTarget, MergeLineAction)>,
) {
    for group in base_only_display_groups(document)
        .into_iter()
        .filter(|group| group.missing_side == side)
    {
        let Some(marker_rect) = merge_base_only_side_rect_from_geometry(document, group, geometry)
        else {
            continue;
        };
        paint_base_only_gap_marker_rect(ui, marker_rect, palette);

        let action_rect = Rect::from_center_size(
            marker_rect.center(),
            Vec2::new(marker_rect.width(), MERGE_CODE_ROW_HEIGHT),
        );
        let action_rects = conflict_action_rects(action_rect, side);
        let action_target = MergeLineActionTarget::BaseOnlyGroup(group.line_index);
        let drop_response = ui.put(action_rects.drop, egui::Button::new("X"));
        if drop_response.clicked() {
            *pending_action = Some((action_target, MergeLineAction::Drop));
        }
        let arrow = match side {
            MergeSide::Local => ">>",
            MergeSide::Remote => "<<",
        };
        let take_response = ui.put(action_rects.take, egui::Button::new(arrow));
        if take_response.clicked() {
            *pending_action = Some((action_target, MergeLineAction::Take));
        }
    }
}

fn merge_side_display_rows(
    document: &MergeDocument,
    side: MergeSide,
) -> Vec<MergeSideDisplayRow<'_>> {
    let mut rows = Vec::new();
    let mut line_index = 0;
    let mut side_line_number = 1;

    while line_index < document.lines.len() {
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            push_conflict_side_display_rows(
                &mut rows,
                conflict,
                side,
                document.conflict_side_resolved(conflict.index, side),
                &mut side_line_number,
            );
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        let line = &document.lines[line_index];
        let raw_missing_side = line.base_only_missing_side_raw();
        if line.base_only_resolved && raw_missing_side == Some(side) {
            line_index += 1;
            continue;
        }

        let missing_side = line.base_only_missing_side();
        if missing_side == Some(side) {
            line_index += base_only_gap_group_len(document, line_index, side).max(1);
            continue;
        }

        let base_only_display = line.is_base_only_display();
        let side_text = match side {
            MergeSide::Local => line.local.as_deref(),
            MergeSide::Remote => line.remote.as_deref(),
        };
        let text = if base_only_display {
            side_text.unwrap_or("")
        } else {
            side_text.unwrap_or(line.result.as_str())
        };
        let line_number = (side_text.is_some()
            || (line.kind != MergeLineKind::Conflict && !base_only_display))
            .then(|| {
                let number = side_line_number;
                side_line_number += 1;
                number
            });
        rows.push(MergeSideDisplayRow {
            text,
            reference_text: line.base.as_deref(),
            line_number,
            conflict_index: line.conflict_index,
            side_resolved: line.side_resolved(side),
            tone: MergeSideLineTone::Unchanged,
            show_conflict_actions: false,
            action_target: None,
        });
        line_index += 1;
    }

    rows
}

fn cached_merge_side_display_rows(
    document: &MergeDocument,
    side: MergeSide,
) -> Vec<CachedMergeSideDisplayRow> {
    merge_side_display_rows(document, side)
        .into_iter()
        .map(|row| CachedMergeSideDisplayRow {
            text: row.text.to_owned(),
            reference_text: row.reference_text.map(str::to_owned),
            line_number: row.line_number,
            conflict_index: row.conflict_index,
            side_resolved: row.side_resolved,
            tone: row.tone,
            show_conflict_actions: row.show_conflict_actions,
            action_target: row.action_target,
        })
        .collect()
}

fn merge_cached_scroll_anchors(
    document: &MergeDocument,
    side: MergeSide,
    result_rows: &[MergeResultDisplayRow<'_>],
    side_rows: &[CachedMergeSideDisplayRow],
) -> Vec<(f32, f32)> {
    let mut result_spans = HashMap::<usize, (usize, usize)>::new();
    for (row_index, row) in result_rows.iter().enumerate() {
        let Some(conflict_index) = row.conflict_index else {
            continue;
        };
        result_spans
            .entry(conflict_index)
            .and_modify(|(_, count)| *count += 1)
            .or_insert((row_index, 1));
    }

    let mut side_spans = HashMap::<usize, (usize, usize)>::new();
    for (row_index, row) in side_rows.iter().enumerate() {
        let Some(conflict_index) = row.conflict_index else {
            continue;
        };
        side_spans
            .entry(conflict_index)
            .and_modify(|(_, count)| *count += 1)
            .or_insert((row_index, 1));
    }

    let conflict_starts = document
        .conflicts()
        .iter()
        .filter_map(|conflict| {
            conflict
                .line_indices
                .first()
                .copied()
                .map(|line_index| (line_index, conflict))
        })
        .collect::<HashMap<_, _>>();
    let mut document_line = 0;
    let mut result_row = 0;
    let mut side_row = 0;
    let mut anchors = vec![(0.0, 0.0)];
    while document_line < document.lines.len() {
        if let Some(conflict) = conflict_starts.get(&document_line) {
            let result_count = result_spans
                .get(&conflict.index)
                .map_or(0, |(_, count)| *count);
            let side_count = side_spans
                .get(&conflict.index)
                .map_or(0, |(_, count)| *count);
            anchors.push((result_row as f32, side_row as f32));
            result_row += result_count;
            side_row += side_count;
            anchors.push((result_row as f32, side_row as f32));
            document_line = conflict
                .line_indices
                .last()
                .map_or(document_line + 1, |last| last + 1);
            continue;
        }

        let line = &document.lines[document_line];
        let result_count = if line.is_base_only_display() {
            1
        } else {
            line.result_lines().len()
        };
        let side_count = usize::from(
            !(line.base_only_resolved && line.base_only_missing_side_raw() == Some(side))
                && line.base_only_missing_side() != Some(side),
        );
        if result_count != side_count {
            anchors.push((result_row as f32, side_row as f32));
        }
        result_row += result_count;
        side_row += side_count;
        if result_count != side_count {
            anchors.push((result_row as f32, side_row as f32));
        }
        document_line += 1;
    }

    anchors.push((result_rows.len() as f32, side_rows.len() as f32));
    anchors.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.total_cmp(&right.1))
    });
    anchors.dedup();
    anchors
}

fn base_only_gap_group_len(document: &MergeDocument, line_index: usize, side: MergeSide) -> usize {
    document.lines[line_index..]
        .iter()
        .take_while(|line| line.base_only_missing_side() == Some(side))
        .count()
}

fn merge_side_display_row_visual_height(row: &MergeSideDisplayRow<'_>) -> usize {
    let _ = row;
    1
}

fn push_conflict_side_display_rows<'a>(
    rows: &mut Vec<MergeSideDisplayRow<'a>>,
    conflict: &'a ConflictBlock,
    side: MergeSide,
    side_resolved: bool,
    side_line_number: &mut usize,
) {
    let side_lines = match side {
        MergeSide::Local => conflict.local.as_slice(),
        MergeSide::Remote => conflict.remote.as_slice(),
    };
    let compare_sides = conflict_prefers_side_comparison(conflict);
    let diff_reference_lines = if compare_sides {
        match side {
            MergeSide::Local => conflict.remote.as_slice(),
            MergeSide::Remote => conflict.local.as_slice(),
        }
    } else {
        conflict.base.as_slice()
    };
    // Word-level emphasis describes each side's change from the common base, not the
    // opposing side's unrelated wording. A pure insertion has no base text at all.
    let word_reference_lines = conflict.base.as_slice();
    let diff_rows = if compare_sides {
        normalize_side_comparison_diff_rows(
            merge_diff_base_to_side(diff_reference_lines, side_lines),
            conflict,
        )
    } else {
        merge_diff_base_to_side(&conflict.base, side_lines)
    };
    let mut show_conflict_actions = true;

    for (row_index, diff_row) in diff_rows.into_iter().enumerate() {
        let (text, line_number, tone) = match diff_row {
            MergeSideDiffRow::Equal(text) => {
                let number = *side_line_number;
                *side_line_number += 1;
                (text, Some(number), MergeSideLineTone::Unchanged)
            }
            MergeSideDiffRow::Deleted(text) => (
                "",
                None,
                side_diff_tone_for_missing_reference(conflict, compare_sides, text),
            ),
            MergeSideDiffRow::Added(text) => {
                let number = *side_line_number;
                *side_line_number += 1;
                (
                    text,
                    Some(number),
                    side_diff_tone_for_side_text(conflict, compare_sides, text),
                )
            }
            MergeSideDiffRow::Replaced(text) => {
                let number = *side_line_number;
                *side_line_number += 1;
                (text, Some(number), MergeSideLineTone::Replaced)
            }
        };
        rows.push(MergeSideDisplayRow {
            text,
            reference_text: (!conflict.base.is_empty())
                .then(|| word_reference_lines.get(row_index).map(String::as_str))
                .flatten(),
            line_number,
            conflict_index: Some(conflict.index),
            side_resolved,
            tone,
            show_conflict_actions,
            action_target: show_conflict_actions
                .then_some(MergeLineActionTarget::Conflict(conflict.index)),
        });
        show_conflict_actions = false;
    }
}

fn conflict_prefers_side_comparison(conflict: &ConflictBlock) -> bool {
    conflict.local != conflict.base
        && conflict.remote != conflict.base
        && conflict.local != conflict.remote
}

fn normalize_side_comparison_diff_rows<'a>(
    rows: Vec<MergeSideDiffRow<'a>>,
    conflict: &ConflictBlock,
) -> Vec<MergeSideDiffRow<'a>> {
    rows.into_iter()
        .filter_map(|row| match row {
            MergeSideDiffRow::Added(text) if !merge_base_contains_line(conflict, text) => {
                Some(MergeSideDiffRow::Replaced(text))
            }
            MergeSideDiffRow::Deleted(text) if !merge_base_contains_line(conflict, text) => None,
            other => Some(other),
        })
        .collect()
}

fn merge_base_contains_line(conflict: &ConflictBlock, text: &str) -> bool {
    conflict.base.iter().any(|base| base == text)
}

fn side_diff_tone_for_missing_reference(
    conflict: &ConflictBlock,
    compare_sides: bool,
    text: &str,
) -> MergeSideLineTone {
    if compare_sides && merge_base_contains_line(conflict, text) {
        MergeSideLineTone::BaseOnly
    } else {
        MergeSideLineTone::Deleted
    }
}

fn side_diff_tone_for_side_text(
    conflict: &ConflictBlock,
    compare_sides: bool,
    text: &str,
) -> MergeSideLineTone {
    if compare_sides && merge_base_contains_line(conflict, text) {
        MergeSideLineTone::BaseOnly
    } else {
        MergeSideLineTone::Added
    }
}

fn merge_diff_base_to_side<'a>(
    base: &'a [String],
    side: &'a [String],
) -> Vec<MergeSideDiffRow<'a>> {
    let mut lcs = vec![vec![0; side.len() + 1]; base.len() + 1];
    for base_index in (0..base.len()).rev() {
        for side_index in (0..side.len()).rev() {
            lcs[base_index][side_index] = if base[base_index] == side[side_index] {
                lcs[base_index + 1][side_index + 1] + 1
            } else {
                lcs[base_index + 1][side_index].max(lcs[base_index][side_index + 1])
            };
        }
    }

    let mut rows = Vec::new();
    let mut base_index = 0;
    let mut side_index = 0;
    while base_index < base.len() && side_index < side.len() {
        if base[base_index] == side[side_index] {
            rows.push(MergeSideDiffRow::Equal(side[side_index].as_str()));
            base_index += 1;
            side_index += 1;
        } else if lcs[base_index + 1][side_index] >= lcs[base_index][side_index + 1] {
            rows.push(MergeSideDiffRow::Deleted(base[base_index].as_str()));
            base_index += 1;
        } else {
            rows.push(MergeSideDiffRow::Added(side[side_index].as_str()));
            side_index += 1;
        }
    }
    while base_index < base.len() {
        rows.push(MergeSideDiffRow::Deleted(base[base_index].as_str()));
        base_index += 1;
    }
    while side_index < side.len() {
        rows.push(MergeSideDiffRow::Added(side[side_index].as_str()));
        side_index += 1;
    }

    collapse_replacement_rows(rows)
}

fn collapse_replacement_rows<'a>(rows: Vec<MergeSideDiffRow<'a>>) -> Vec<MergeSideDiffRow<'a>> {
    let mut collapsed = Vec::new();
    let mut index = 0;

    while index < rows.len() {
        if !matches!(rows[index], MergeSideDiffRow::Deleted(_)) {
            collapsed.push(rows[index]);
            index += 1;
            continue;
        }

        let delete_start = index;
        while index < rows.len() && matches!(rows[index], MergeSideDiffRow::Deleted(_)) {
            index += 1;
        }
        let add_start = index;
        while index < rows.len() && matches!(rows[index], MergeSideDiffRow::Added(_)) {
            index += 1;
        }

        if add_start == index {
            collapsed.extend_from_slice(&rows[delete_start..add_start]);
            continue;
        }

        let deleted = &rows[delete_start..add_start];
        let added = &rows[add_start..index];
        let replace_count = deleted.len().min(added.len());
        for row in added.iter().take(replace_count) {
            if let MergeSideDiffRow::Added(text) = row {
                collapsed.push(MergeSideDiffRow::Replaced(text));
            }
        }
        if deleted.len() > replace_count {
            collapsed.extend_from_slice(&deleted[replace_count..]);
        }
        if added.len() > replace_count {
            collapsed.extend_from_slice(&added[replace_count..]);
        }
    }

    collapsed
}

fn merge_editable_result_row(
    ui: &mut Ui,
    index: usize,
    text: &mut String,
    tone: MergeSideLineTone,
    background: Option<(Color32, usize)>,
    highlight_mode: MergeHighlightMode,
    reference_text: Option<&str>,
    palette: MergePalette,
) -> (Rect, Option<String>) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), MERGE_CODE_ROW_HEIGHT),
        Sense::hover(),
    );
    if let Some((fill, row_count)) = background {
        // Paint one solid span before any row text. Per-row fills can leave a one-pixel
        // antialiasing seam between adjacent conflict rows on fractional display scales.
        let fill_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(
                rect.right(),
                rect.bottom() + MERGE_CODE_ROW_HEIGHT * row_count.saturating_sub(1) as f32,
            ),
        );
        ui.painter()
            .rect_filled(fill_rect, egui::CornerRadius::ZERO, fill);
    }
    ui.painter().text(
        Pos2::new(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{:>4}", index + 1),
        FontId::monospace(MERGE_CODE_FONT_SIZE),
        palette.muted,
    );
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 62.0, rect.top()),
        rect.right_bottom(),
    );
    let text_color = match tone {
        MergeSideLineTone::Added => palette.added_text,
        MergeSideLineTone::BaseOnly => palette.base_only_text,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced => palette.conflict_text,
        MergeSideLineTone::Unchanged => palette.text,
    };
    if highlight_mode == MergeHighlightMode::Words
        && tone != MergeSideLineTone::Unchanged
        && !text.is_empty()
    {
        paint_word_highlight_backgrounds(
            ui,
            text_rect,
            text,
            reference_text.unwrap_or(""),
            text_color,
        );
    }
    let before = text.clone();
    let changed = ui
        .put(
            text_rect,
            egui::TextEdit::singleline(text)
                .id_salt(("merge_result_line", index))
                .frame(false)
                // Match the word-highlight coordinate system. TextEdit otherwise inserts 4px.
                .margin(egui::Margin::ZERO)
                .font(FontId::monospace(MERGE_CODE_FONT_SIZE))
                .text_color(text_color)
                .desired_width(text_rect.width()),
        )
        .changed()
        .then_some(before);
    (rect, changed)
}

fn merge_result_row_fill(
    tone: MergeSideLineTone,
    active_conflict: bool,
    palette: MergePalette,
) -> Color32 {
    match tone {
        MergeSideLineTone::Added => palette.added_fill,
        MergeSideLineTone::BaseOnly => palette.base_only_fill,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced => {
            if active_conflict {
                palette.active_conflict_fill
            } else {
                palette.conflict_fill
            }
        }
        MergeSideLineTone::Unchanged => palette.result_fill,
    }
}

fn merge_highlight_fill(
    tone: MergeSideLineTone,
    fill: Color32,
    active_conflict: bool,
    highlight_mode: MergeHighlightMode,
) -> Color32 {
    match highlight_mode {
        MergeHighlightMode::Lines | MergeHighlightMode::Words
            if tone == MergeSideLineTone::BaseOnly =>
        {
            // Base-only represents a deletion decision. It stays a full grey block in word
            // mode so the actionable deletion does not fade into surrounding context.
            fill
        }
        MergeHighlightMode::Lines => fill,
        MergeHighlightMode::Words => color_with_opacity(
            fill,
            if active_conflict {
                MERGE_WORD_ACTIVE_BLOCK_OPACITY
            } else {
                MERGE_WORD_BLOCK_OPACITY
            },
        ),
    }
}

fn merge_result_background_run(
    styles: &[(MergeSideLineTone, bool)],
    index: usize,
    visible_start: usize,
    visible_end: usize,
    highlight_mode: MergeHighlightMode,
    palette: MergePalette,
) -> Option<(Color32, usize)> {
    let &(tone, active) = styles.get(index)?;
    let fill = merge_result_row_fill(tone, active, palette);
    let fill = merge_highlight_fill(tone, fill, active, highlight_mode);
    let is_start = index == visible_start
        || styles
            .get(index.saturating_sub(1))
            .is_none_or(|&(previous_tone, previous_active)| {
                merge_highlight_fill(
                    previous_tone,
                    merge_result_row_fill(previous_tone, previous_active, palette),
                    previous_active,
                    highlight_mode,
                ) != fill
            });
    if !is_start {
        return None;
    }

    let mut count = 1;
    while index + count < visible_end {
        let (next_tone, next_active) = styles[index + count];
        let next_fill = merge_highlight_fill(
            next_tone,
            merge_result_row_fill(next_tone, next_active, palette),
            next_active,
            highlight_mode,
        );
        if next_fill != fill {
            break;
        }
        count += 1;
    }
    Some((fill, count))
}

#[cfg(test)]
fn merge_result_display_lines(document: &MergeDocument) -> Vec<&str> {
    merge_result_display_rows(document)
        .into_iter()
        .map(|row| row.text)
        .collect()
}

fn merge_result_display_rows(document: &MergeDocument) -> Vec<MergeResultDisplayRow<'_>> {
    let mut rows = Vec::new();
    let mut line_index = 0;

    while line_index < document.lines.len() {
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            push_conflict_result_display_rows(&mut rows, document, conflict);
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        let line = &document.lines[line_index];
        if line.is_base_only_display() {
            rows.push(MergeResultDisplayRow {
                text: line.result.as_str(),
                reference_text: line.base.as_deref(),
                conflict_index: None,
                tone: MergeSideLineTone::BaseOnly,
            });
        } else {
            for text in line.result_lines() {
                rows.push(MergeResultDisplayRow {
                    text,
                    reference_text: line.base.as_deref(),
                    conflict_index: None,
                    tone: MergeSideLineTone::Unchanged,
                });
            }
        }
        line_index += 1;
    }

    rows
}

fn push_conflict_result_display_rows<'a>(
    rows: &mut Vec<MergeResultDisplayRow<'a>>,
    document: &'a MergeDocument,
    conflict: &'a ConflictBlock,
) {
    let selected = conflict
        .line_indices
        .iter()
        .filter_map(|line_index| document.lines.get(*line_index))
        .flat_map(MergeLine::result_lines)
        .collect::<Vec<_>>();
    if !selected.is_empty() {
        for (index, text) in selected.into_iter().enumerate() {
            rows.push(MergeResultDisplayRow {
                text,
                reference_text: conflict.base.get(index).map(String::as_str),
                conflict_index: Some(conflict.index),
                tone: MergeSideLineTone::Unchanged,
            });
        }
        return;
    }

    if document.conflict_fully_resolved(conflict.index) {
        return;
    }

    if conflict.base.is_empty() {
        // An insertion conflict lives on the boundary between two result rows. Keeping a
        // blank display row here shifts everything below it out of alignment, so the painter
        // draws a narrow marker at that boundary from the real row geometry instead.
        return;
    }

    let tones = merge_base_result_tones(conflict);
    for (index, (text, tone)) in conflict.base.iter().zip(tones).enumerate() {
        rows.push(MergeResultDisplayRow {
            text,
            reference_text: conflict
                .local
                .get(index)
                .or_else(|| conflict.remote.get(index))
                .map(String::as_str),
            conflict_index: Some(conflict.index),
            tone,
        });
    }
}

fn merge_base_result_tones(conflict: &ConflictBlock) -> Vec<MergeSideLineTone> {
    let local_kept = merge_base_lines_kept_by_side(&conflict.base, &conflict.local);
    let remote_kept = merge_base_lines_kept_by_side(&conflict.base, &conflict.remote);
    let has_local_only_base = local_kept
        .iter()
        .zip(remote_kept.iter())
        .any(|(local, remote)| *local && !*remote);
    let has_remote_only_base = local_kept
        .iter()
        .zip(remote_kept.iter())
        .any(|(local, remote)| !*local && *remote);
    let opposing_base_deletions = has_local_only_base && has_remote_only_base;

    local_kept
        .iter()
        .zip(remote_kept.iter())
        .map(|(local, remote)| match (*local, *remote) {
            (true, true) => MergeSideLineTone::Unchanged,
            (true, false) | (false, true) if opposing_base_deletions => MergeSideLineTone::Replaced,
            (true, false) | (false, true) => MergeSideLineTone::BaseOnly,
            (false, false) => MergeSideLineTone::Replaced,
        })
        .collect()
}

fn merge_base_lines_kept_by_side(base: &[String], side: &[String]) -> Vec<bool> {
    let mut lcs = vec![vec![0; side.len() + 1]; base.len() + 1];
    for base_index in (0..base.len()).rev() {
        for side_index in (0..side.len()).rev() {
            lcs[base_index][side_index] = if base[base_index] == side[side_index] {
                lcs[base_index + 1][side_index + 1] + 1
            } else {
                lcs[base_index + 1][side_index].max(lcs[base_index][side_index + 1])
            };
        }
    }

    let mut kept = vec![false; base.len()];
    let mut base_index = 0;
    let mut side_index = 0;
    while base_index < base.len() && side_index < side.len() {
        if base[base_index] == side[side_index] {
            kept[base_index] = true;
            base_index += 1;
            side_index += 1;
        } else if lcs[base_index + 1][side_index] >= lcs[base_index][side_index + 1] {
            base_index += 1;
        } else {
            side_index += 1;
        }
    }
    kept
}

fn paint_merge_block_connectors(
    ui: &Ui,
    document: &MergeDocument,
    local_geometry: &MergePanelGeometry,
    result_geometry: &MergePanelGeometry,
    remote_geometry: &MergePanelGeometry,
    debug: MergeConnectorDebug,
    palette: MergePalette,
) {
    for conflict in document.conflicts() {
        let tone = if conflict.base.is_empty() {
            MergeSideLineTone::Replaced
        } else {
            merge_block_connector_tone(document, conflict)
        };
        let result_rect = if conflict.base.is_empty() {
            merge_conflict_insertion_marker_rect(document, conflict, result_geometry)
        } else {
            merge_block_result_rect_from_geometry(document, conflict, result_geometry)
        };
        let Some(result_rect) = result_rect else {
            continue;
        };
        if conflict.base.is_empty() {
            // Zero-width conflicts do not own a result row. Paint the marker directly on the
            // boundary so the line remains visible without shifting later rows downward.
            ui.painter().rect_filled(
                result_rect,
                egui::CornerRadius::ZERO,
                merge_connector_fill(tone, palette),
            );
        }
        paint_result_block_outline(ui, result_rect, tone, palette);

        if let Some(local_rect) = merge_block_side_rect_from_geometry(
            document,
            conflict,
            MergeSide::Local,
            local_geometry,
        ) {
            paint_side_block_bridge(ui, result_rect, local_rect, MergeSide::Local, tone, palette);
            paint_side_block_debug(
                ui,
                debug,
                "conflict",
                conflict.index,
                MergeSide::Local,
                result_rect,
                local_rect,
                tone,
            );
        }
        if let Some(remote_rect) = merge_block_side_rect_from_geometry(
            document,
            conflict,
            MergeSide::Remote,
            remote_geometry,
        ) {
            paint_side_block_bridge(
                ui,
                result_rect,
                remote_rect,
                MergeSide::Remote,
                tone,
                palette,
            );
            paint_side_block_debug(
                ui,
                debug,
                "conflict",
                conflict.index,
                MergeSide::Remote,
                result_rect,
                remote_rect,
                tone,
            );
        }
    }

    for group in base_only_display_groups(document) {
        let Some(result_rect) =
            merge_base_only_result_rect_from_geometry(document, group, result_geometry)
        else {
            continue;
        };
        let tone = MergeSideLineTone::BaseOnly;
        paint_result_block_outline(ui, result_rect, tone, palette);
        let side_geometry = match group.missing_side {
            MergeSide::Local => local_geometry,
            MergeSide::Remote => remote_geometry,
        };
        if let Some(side_rect) =
            merge_base_only_side_rect_from_geometry(document, group, side_geometry)
        {
            paint_base_only_marker_bridge(ui, result_rect, side_rect, group.missing_side, palette);
            paint_side_block_debug(
                ui,
                debug,
                "base-only",
                group.line_index,
                group.missing_side,
                result_rect,
                side_rect,
                tone,
            );
        }
    }
}

fn merge_connector_debug_mode() -> MergeConnectorDebug {
    if merge_build_config_bool("connector_log") {
        MergeConnectorDebug::Log
    } else if merge_build_config_bool("connector_guides") {
        MergeConnectorDebug::Guides
    } else {
        MergeConnectorDebug::Off
    }
}

fn merge_build_config_bool(key: &str) -> bool {
    MERGE_BUILD_CONFIG
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then_some(value.trim())
        })
        .is_some_and(|value| matches!(value, "true" | "1" | "yes" | "on"))
}

fn base_only_display_groups(document: &MergeDocument) -> Vec<BaseOnlyDisplayGroup> {
    let mut groups = Vec::new();
    let mut line_index = 0;
    while line_index < document.lines.len() {
        let Some(missing_side) = document.lines[line_index].base_only_missing_side() else {
            line_index += 1;
            continue;
        };
        let line_count = base_only_gap_group_len(document, line_index, missing_side).max(1);
        groups.push(BaseOnlyDisplayGroup {
            line_index,
            line_count,
            missing_side,
        });
        line_index += line_count;
    }
    groups
}

fn merge_block_connector_tone(
    document: &MergeDocument,
    conflict: &ConflictBlock,
) -> MergeSideLineTone {
    let mut has_base_only = false;
    let mut has_added = false;

    for row in merge_result_display_rows(document)
        .into_iter()
        .filter(|row| row.conflict_index == Some(conflict.index))
    {
        match row.tone {
            MergeSideLineTone::Replaced | MergeSideLineTone::Deleted => {
                return MergeSideLineTone::Replaced;
            }
            MergeSideLineTone::BaseOnly => has_base_only = true,
            MergeSideLineTone::Added => has_added = true,
            MergeSideLineTone::Unchanged => {}
        }
    }

    if has_base_only {
        MergeSideLineTone::BaseOnly
    } else if has_added {
        MergeSideLineTone::Added
    } else {
        MergeSideLineTone::Unchanged
    }
}

fn merge_block_result_rect_from_geometry(
    document: &MergeDocument,
    conflict: &ConflictBlock,
    geometry: &MergePanelGeometry,
) -> Option<Rect> {
    let (first, count) = merge_result_row_span_for_conflict(document, conflict)?;
    geometry.span_rect(first, count)
}

fn merge_block_side_rect_from_geometry(
    document: &MergeDocument,
    conflict: &ConflictBlock,
    side: MergeSide,
    geometry: &MergePanelGeometry,
) -> Option<Rect> {
    let (first, count) = merge_side_row_span_for_conflict(document, side, conflict)?;
    geometry.span_rect(first, count)
}

fn merge_conflict_insertion_marker_rect(
    document: &MergeDocument,
    conflict: &ConflictBlock,
    geometry: &MergePanelGeometry,
) -> Option<Rect> {
    let first_line = *conflict.line_indices.first()?;
    let row_index = merge_result_display_boundary_before_line(document, first_line);
    geometry.boundary_marker_rect(row_index, MERGE_BASE_ONLY_MARKER_HEIGHT)
}

fn merge_result_scroll_y_for_conflict(
    document: &MergeDocument,
    conflict_index: usize,
) -> Option<f32> {
    let conflict = document
        .conflicts()
        .iter()
        .find(|conflict| conflict.index == conflict_index)?;
    let row_index = merge_result_row_span_for_conflict(document, conflict)
        .map(|(first, _)| first)
        .unwrap_or_else(|| {
            conflict
                .line_indices
                .first()
                .map(|first_line| merge_result_display_boundary_before_line(document, *first_line))
                .unwrap_or_else(|| merge_result_display_rows(document).len())
        });
    Some(row_index as f32 * MERGE_CODE_ROW_HEIGHT)
}

fn merge_result_display_boundary_before_line(
    document: &MergeDocument,
    target_line_index: usize,
) -> usize {
    let mut display_row = 0;
    let mut line_index = 0;
    while line_index < document.lines.len() {
        if line_index == target_line_index {
            return display_row;
        }
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            display_row += merge_result_row_span_for_conflict(document, conflict)
                .map_or(0, |(_, count)| count);
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        let line = &document.lines[line_index];
        display_row += if line.is_base_only_display() {
            1
        } else {
            line.result_lines().len()
        };
        line_index += 1;
    }
    display_row
}

fn merge_base_only_result_rect_from_geometry(
    document: &MergeDocument,
    group: BaseOnlyDisplayGroup,
    geometry: &MergePanelGeometry,
) -> Option<Rect> {
    let first = merge_result_display_row_for_line(document, group.line_index)?;
    geometry.span_rect(first, group.line_count)
}

fn merge_base_only_side_rect_from_geometry(
    document: &MergeDocument,
    group: BaseOnlyDisplayGroup,
    geometry: &MergePanelGeometry,
) -> Option<Rect> {
    let row_index =
        merge_side_display_row_for_line(document, group.missing_side, group.line_index)?;
    geometry.boundary_marker_rect(row_index, MERGE_BASE_ONLY_MARKER_HEIGHT)
}

fn merge_block_result_rect(
    result_panel: Rect,
    document: &MergeDocument,
    conflict: &ConflictBlock,
    scroll_y: f32,
) -> Option<Rect> {
    let (display_row, display_count) = merge_result_row_span_for_conflict(document, conflict)?;
    let clip = merge_scroll_clip_rect(result_panel);
    let top = merge_scroll_content_top(result_panel) + display_row as f32 * MERGE_CODE_ROW_HEIGHT
        - scroll_y;
    let bottom = top + display_count as f32 * MERGE_CODE_ROW_HEIGHT;
    if bottom <= clip.top() || top >= clip.bottom() {
        return None;
    }
    Some(Rect::from_min_max(
        Pos2::new(clip.left(), top.max(clip.top())),
        Pos2::new(clip.right(), bottom.min(clip.bottom())),
    ))
}

fn merge_result_row_span_for_conflict(
    document: &MergeDocument,
    conflict: &ConflictBlock,
) -> Option<(usize, usize)> {
    let rows = merge_result_display_rows(document);
    let first = rows
        .iter()
        .position(|row| row.conflict_index == Some(conflict.index))?;
    let count = rows
        .iter()
        .skip(first)
        .take_while(|row| row.conflict_index == Some(conflict.index))
        .count();
    (count > 0).then_some((first, count))
}

fn merge_block_side_rect(
    side_panel: Rect,
    document: &MergeDocument,
    conflict: &ConflictBlock,
    side: MergeSide,
    scroll_y: f32,
) -> Option<Rect> {
    let (first, count) = merge_side_row_span_for_conflict(document, side, conflict)?;

    let clip = merge_scroll_clip_rect(side_panel);
    let top =
        merge_scroll_content_top(side_panel) + first as f32 * MERGE_CODE_ROW_HEIGHT - scroll_y;
    let bottom = top + count as f32 * MERGE_CODE_ROW_HEIGHT;
    if bottom <= clip.top() || top >= clip.bottom() {
        return None;
    }
    Some(Rect::from_min_max(
        Pos2::new(side_panel.left() + 6.0, top.max(clip.top())),
        Pos2::new(side_panel.right() - 6.0, bottom.min(clip.bottom())),
    ))
}

fn merge_base_only_result_rect(
    result_panel: Rect,
    document: &MergeDocument,
    group: BaseOnlyDisplayGroup,
    scroll_y: f32,
) -> Option<Rect> {
    let display_row = merge_result_display_row_for_line(document, group.line_index)?;
    let clip = merge_scroll_clip_rect(result_panel);
    let top = merge_scroll_content_top(result_panel) + display_row as f32 * MERGE_CODE_ROW_HEIGHT
        - scroll_y;
    let bottom = top + group.line_count as f32 * MERGE_CODE_ROW_HEIGHT;
    if bottom <= clip.top() || top >= clip.bottom() {
        return None;
    }
    Some(Rect::from_min_max(
        Pos2::new(clip.left(), top.max(clip.top())),
        Pos2::new(clip.right(), bottom.min(clip.bottom())),
    ))
}

fn merge_base_only_side_rect(
    side_panel: Rect,
    document: &MergeDocument,
    group: BaseOnlyDisplayGroup,
    scroll_y: f32,
) -> Option<Rect> {
    let boundary_row =
        merge_side_display_row_for_line(document, group.missing_side, group.line_index)?;
    let clip = merge_scroll_clip_rect(side_panel);
    let top = merge_scroll_content_top(side_panel) + boundary_row as f32 * MERGE_CODE_ROW_HEIGHT
        - scroll_y;
    let bottom = top;
    let row_rect = Rect::from_min_max(Pos2::new(clip.left(), top), Pos2::new(clip.right(), bottom));
    let marker_rect = base_only_gap_marker_rect(row_rect);
    if marker_rect.bottom() <= clip.top() || marker_rect.top() >= clip.bottom() {
        return None;
    }
    Some(marker_rect)
}

fn merge_side_scroll_y_for_result_scroll(
    document: &MergeDocument,
    side: MergeSide,
    result_scroll_y: f32,
) -> f32 {
    let result_row = result_scroll_y / MERGE_CODE_ROW_HEIGHT;
    merge_mapped_scroll_row(&merge_scroll_anchors(document, side), result_row, true)
        * MERGE_CODE_ROW_HEIGHT
}

fn merge_result_scroll_y_for_side_scroll(
    document: &MergeDocument,
    side: MergeSide,
    side_scroll_y: f32,
) -> f32 {
    let side_row = side_scroll_y / MERGE_CODE_ROW_HEIGHT;
    merge_mapped_scroll_row(&merge_scroll_anchors(document, side), side_row, false)
        * MERGE_CODE_ROW_HEIGHT
}

fn merge_mapped_scroll_row(anchors: &[(f32, f32)], source_row: f32, result_to_side: bool) -> f32 {
    if anchors.is_empty() {
        return source_row.max(0.0);
    }
    let project = |anchor: (f32, f32)| {
        if result_to_side {
            (anchor.0, anchor.1)
        } else {
            (anchor.1, anchor.0)
        }
    };
    let mut points = anchors.iter().copied().map(project).collect::<Vec<_>>();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));

    if source_row <= points[0].0 {
        return (points[0].1 + source_row - points[0].0).max(0.0);
    }

    for pair in points.windows(2) {
        let (source_a, target_a) = pair[0];
        let (source_b, target_b) = pair[1];
        if source_row <= source_b {
            let source_span = (source_b - source_a).max(1.0);
            let t = ((source_row - source_a) / source_span).clamp(0.0, 1.0);
            return target_a + (target_b - target_a) * t;
        }
    }

    let (source_last, target_last) = points[points.len() - 1];
    (target_last + source_row - source_last).max(0.0)
}

fn merge_scroll_anchors(document: &MergeDocument, side: MergeSide) -> Vec<(f32, f32)> {
    let mut anchors = Vec::new();
    let mut line_index = 0;
    while line_index < document.lines.len() {
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            if let (Some((result_row, _)), Some((side_row, _))) = (
                merge_result_row_span_for_conflict(document, conflict),
                merge_side_row_span_for_conflict(document, side, conflict),
            ) {
                anchors.push((result_row as f32, side_row as f32));
            }
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        if let (Some(result_row), Some(side_row)) = (
            merge_result_display_row_for_line(document, line_index),
            merge_side_display_row_for_line(document, side, line_index),
        ) {
            anchors.push((result_row as f32, side_row as f32));
        }
        line_index += 1;
    }
    anchors
}

fn merge_side_row_span_for_conflict(
    document: &MergeDocument,
    side: MergeSide,
    conflict: &ConflictBlock,
) -> Option<(usize, usize)> {
    let rows = merge_side_display_rows(document, side);
    let mut visual_row = 0;
    let mut first = None;
    let mut count = 0;
    for row in &rows {
        let visual_height = merge_side_display_row_visual_height(row);
        if row.conflict_index == Some(conflict.index) {
            first.get_or_insert(visual_row);
            count += visual_height;
        } else if first.is_some() {
            break;
        }
        visual_row += visual_height;
    }
    first.and_then(|first| (count > 0).then_some((first, count)))
}

fn merge_side_display_row_for_line(
    document: &MergeDocument,
    side: MergeSide,
    target_line_index: usize,
) -> Option<usize> {
    let mut display_row = 0;
    let mut line_index = 0;
    while line_index < document.lines.len() {
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            if conflict.line_indices.contains(&target_line_index) {
                return Some(display_row);
            }
            display_row += merge_side_row_span_for_conflict(document, side, conflict)
                .map_or(0, |(_, count)| count);
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        let line = &document.lines[line_index];
        let raw_missing_side = line.base_only_missing_side_raw();
        if line.base_only_resolved && raw_missing_side == Some(side) {
            if line_index == target_line_index {
                return None;
            }
            line_index += 1;
            continue;
        }

        let missing_side = line.base_only_missing_side();
        if missing_side == Some(side) {
            let group_len = base_only_gap_group_len(document, line_index, side).max(1);
            if (line_index..line_index + group_len).contains(&target_line_index) {
                return Some(display_row);
            }
            line_index += group_len;
            continue;
        }

        if line_index == target_line_index {
            return Some(display_row);
        }
        display_row += 1;
        line_index += 1;
    }
    None
}

fn merge_result_display_row_for_line(
    document: &MergeDocument,
    target_line_index: usize,
) -> Option<usize> {
    let mut display_row = 0;
    let mut line_index = 0;
    while line_index < document.lines.len() {
        if let Some(conflict) = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.line_indices.first().copied() == Some(line_index))
        {
            if conflict.line_indices.contains(&target_line_index) {
                return None;
            }
            display_row += merge_result_row_span_for_conflict(document, conflict)
                .map_or(0, |(_, count)| count);
            line_index = conflict
                .line_indices
                .last()
                .map_or(line_index + 1, |last| last + 1);
            continue;
        }

        if line_index == target_line_index {
            return Some(display_row);
        }
        let line = &document.lines[line_index];
        display_row += if line.is_base_only_display() {
            1
        } else {
            line.result_lines().len()
        };
        line_index += 1;
    }
    None
}

fn merge_scroll_content_top(panel: Rect) -> f32 {
    panel.top() + 6.0 + 24.0 + MERGE_NAV_BUTTON_SIZE + 8.0
}

fn merge_scroll_clip_rect(panel: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(panel.left() + 6.0, merge_scroll_content_top(panel)),
        Pos2::new(panel.right() - 6.0, panel.bottom() - 6.0),
    )
}

fn paint_result_block_outline(ui: &Ui, rect: Rect, tone: MergeSideLineTone, palette: MergePalette) {
    if !should_paint_result_block_outline(tone) {
        return;
    }
    let stroke = egui::Stroke::new(1.2, merge_connector_color(tone, palette));
    ui.painter()
        .line_segment([rect.left_top(), rect.right_top()], stroke);
    ui.painter()
        .line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
}

fn should_paint_result_block_outline(tone: MergeSideLineTone) -> bool {
    matches!(tone, MergeSideLineTone::Added)
}

fn paint_side_block_bridge(
    ui: &Ui,
    result_rect: Rect,
    side_rect: Rect,
    side: MergeSide,
    tone: MergeSideLineTone,
    palette: MergePalette,
) {
    let (result_x, side_x) = match side {
        MergeSide::Local => (result_rect.left(), side_rect.right()),
        MergeSide::Remote => (result_rect.right(), side_rect.left()),
    };
    let knee_x = (result_x + side_x) * 0.5;
    let fill = merge_connector_fill(tone, palette);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(result_x, result_rect.top()),
            Pos2::new(knee_x, result_rect.top()),
            Pos2::new(side_x, side_rect.top()),
            Pos2::new(side_x, side_rect.bottom()),
            Pos2::new(knee_x, result_rect.bottom()),
            Pos2::new(result_x, result_rect.bottom()),
        ],
        fill,
        egui::Stroke::NONE,
    ));
}

fn paint_base_only_marker_bridge(
    ui: &Ui,
    result_rect: Rect,
    marker_rect: Rect,
    side: MergeSide,
    palette: MergePalette,
) {
    ui.painter().add(egui::Shape::convex_polygon(
        base_only_marker_bridge_points(result_rect, marker_rect, side),
        merge_connector_fill(MergeSideLineTone::BaseOnly, palette),
        egui::Stroke::NONE,
    ));
}

fn base_only_marker_bridge_points(
    result_rect: Rect,
    marker_rect: Rect,
    side: MergeSide,
) -> Vec<Pos2> {
    let (result_x, side_x) = match side {
        MergeSide::Local => (result_rect.left(), marker_rect.right()),
        MergeSide::Remote => (result_rect.right(), marker_rect.left()),
    };
    let knee_x = (result_x + side_x) * 0.5;
    vec![
        Pos2::new(result_x, result_rect.top()),
        Pos2::new(knee_x, result_rect.top()),
        Pos2::new(side_x, marker_rect.top()),
        Pos2::new(side_x, marker_rect.bottom()),
        Pos2::new(knee_x, result_rect.bottom()),
        Pos2::new(result_x, result_rect.bottom()),
    ]
}

fn paint_side_block_debug(
    ui: &Ui,
    mode: MergeConnectorDebug,
    kind: &str,
    index: usize,
    side: MergeSide,
    result_rect: Rect,
    side_rect: Rect,
    tone: MergeSideLineTone,
) {
    if mode == MergeConnectorDebug::Off {
        return;
    }

    let painter = ui.painter();
    let side_color = Color32::from_rgb(245, 158, 11);
    let result_color = Color32::from_rgb(37, 99, 235);
    let side_stroke = egui::Stroke::new(2.0, side_color);
    let result_stroke = egui::Stroke::new(2.0, result_color);
    painter.line_segment([side_rect.left_top(), side_rect.right_top()], side_stroke);
    painter.line_segment(
        [side_rect.left_bottom(), side_rect.right_bottom()],
        side_stroke,
    );
    painter.line_segment(
        [result_rect.left_top(), result_rect.right_top()],
        result_stroke,
    );
    painter.line_segment(
        [result_rect.left_bottom(), result_rect.right_bottom()],
        result_stroke,
    );

    let (result_x, side_x) = match side {
        MergeSide::Local => (result_rect.left(), side_rect.right()),
        MergeSide::Remote => (result_rect.right(), side_rect.left()),
    };
    let guide_stroke = egui::Stroke::new(2.0, Color32::from_rgb(220, 38, 38));
    painter.line_segment(
        [
            Pos2::new(side_x, side_rect.top()),
            Pos2::new(result_x, result_rect.top()),
        ],
        guide_stroke,
    );
    painter.line_segment(
        [
            Pos2::new(side_x, side_rect.bottom()),
            Pos2::new(result_x, result_rect.bottom()),
        ],
        guide_stroke,
    );

    if mode == MergeConnectorDebug::Log {
        let count = MERGE_CONNECTOR_DEBUG_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
        if count < 200 {
            eprintln!(
                "merge-connector {kind}#{index} side={side:?} tone={tone:?} \
                 side=({:.1},{:.1}) result=({:.1},{:.1}) \
                 delta_top={:.1} delta_bottom={:.1}",
                side_rect.top(),
                side_rect.bottom(),
                result_rect.top(),
                result_rect.bottom(),
                result_rect.top() - side_rect.top(),
                result_rect.bottom() - side_rect.bottom(),
            );
        }
    }
}

fn merge_connector_color(tone: MergeSideLineTone, palette: MergePalette) -> Color32 {
    match tone {
        MergeSideLineTone::Added => palette.connector,
        MergeSideLineTone::BaseOnly => palette.base_only_text,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced => palette.conflict_text,
        MergeSideLineTone::Unchanged => palette.connector,
    }
}

fn merge_connector_fill(tone: MergeSideLineTone, palette: MergePalette) -> Color32 {
    let fill = match tone {
        MergeSideLineTone::Added => palette.added_fill,
        MergeSideLineTone::BaseOnly => palette.base_only_connector_fill,
        MergeSideLineTone::Deleted | MergeSideLineTone::Replaced => palette.conflict_fill,
        MergeSideLineTone::Unchanged => return Color32::TRANSPARENT,
    };
    color_with_opacity(fill, 0.9)
}

fn color_with_opacity(color: Color32, opacity: f32) -> Color32 {
    let alpha = (color.a() as f32 * opacity).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn conflict_action_rects(rect: Rect, side: MergeSide) -> ConflictActionRects {
    let top = rect.top();
    let drop_size = Vec2::new(18.0, MERGE_CODE_ROW_HEIGHT);
    let take_size = Vec2::new(28.0, MERGE_CODE_ROW_HEIGHT);
    let first_left = rect.left() + 4.0;
    let second_left = first_left + take_size.x + 2.0;
    match side {
        MergeSide::Local => ConflictActionRects {
            drop: Rect::from_min_size(Pos2::new(first_left, top), drop_size),
            take: Rect::from_min_size(Pos2::new(first_left + drop_size.x + 2.0, top), take_size),
        },
        MergeSide::Remote => ConflictActionRects {
            take: Rect::from_min_size(Pos2::new(first_left, top), take_size),
            drop: Rect::from_min_size(Pos2::new(second_left, top), drop_size),
        },
    }
}

fn previous_unresolved_conflict(document: &MergeDocument, side: MergeSide, cursor: &mut usize) {
    let unresolved = document
        .conflicts()
        .iter()
        .map(|conflict| conflict.index)
        .filter(|index| !document.conflict_side_resolved(*index, side))
        .collect::<Vec<_>>();
    if let Some(index) = unresolved
        .iter()
        .copied()
        .rev()
        .find(|index| *index < *cursor)
        .or_else(|| unresolved.last().copied())
    {
        *cursor = index;
    }
}

fn next_unresolved_conflict(document: &MergeDocument, side: MergeSide, cursor: &mut usize) {
    let unresolved = document
        .conflicts()
        .iter()
        .map(|conflict| conflict.index)
        .filter(|index| !document.conflict_side_resolved(*index, side))
        .collect::<Vec<_>>();
    if let Some(index) = unresolved
        .iter()
        .copied()
        .find(|index| *index > *cursor)
        .or_else(|| unresolved.first().copied())
    {
        *cursor = index;
    }
}

fn unresolved_position(document: &MergeDocument, side: MergeSide, cursor: usize) -> usize {
    document
        .conflicts()
        .iter()
        .filter(|conflict| !document.conflict_side_resolved(conflict.index, side))
        .position(|conflict| conflict.index == cursor)
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn merge_panel_frame(ui: &mut Ui, palette: MergePalette, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(palette.panel)
        .shadow(palette.shadow)
        .corner_radius(egui::CornerRadius::same(MERGE_PANEL_RADIUS))
        .inner_margin(egui::Margin::symmetric(6, 6))
        .show(ui, body);
}

fn apply_merge_theme(ctx: &egui::Context, theme: MergeTheme) {
    let palette = merge_palette(theme);
    let mut visuals = match theme {
        MergeTheme::Dark => egui::Visuals::dark(),
        MergeTheme::Light => egui::Visuals::light(),
    };
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.panel;
    visuals.extreme_bg_color = palette.panel_soft;
    visuals.faint_bg_color = palette.panel_soft;
    visuals.override_text_color = Some(palette.text);
    visuals.selection.bg_fill = palette.accent;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.open.bg_stroke = egui::Stroke::NONE;
    ctx.set_visuals(visuals);
}

fn merge_palette(theme: MergeTheme) -> MergePalette {
    match theme {
        MergeTheme::Dark => MergePalette {
            bg: Color32::from_rgb(24, 27, 31),
            panel: Color32::from_rgb(29, 32, 36),
            panel_soft: Color32::from_rgb(49, 43, 43),
            text: Color32::from_rgb(222, 229, 238),
            muted: Color32::from_rgb(130, 143, 160),
            accent: Color32::from_rgb(57, 120, 220),
            conflict_fill: Color32::from_rgb(70, 47, 43),
            active_conflict_fill: Color32::from_rgb(104, 57, 48),
            conflict_text: Color32::from_rgb(255, 190, 170),
            added_fill: Color32::from_rgb(42, 68, 52),
            added_text: Color32::from_rgb(150, 226, 170),
            base_only_fill: Color32::from_rgb(81, 85, 92),
            base_only_connector_fill: Color32::from_rgb(81, 85, 92),
            base_only_text: Color32::from_rgb(192, 200, 210),
            connector: Color32::from_rgb(92, 145, 98),
            result_fill: Color32::from_rgb(31, 34, 38),
            shadow: eframe::epaint::Shadow {
                offset: [3, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_rgba_unmultiplied(0, 0, 0, 90),
            },
        },
        MergeTheme::Light => MergePalette {
            bg: Color32::from_rgb(239, 242, 246),
            panel: Color32::from_rgb(253, 254, 255),
            panel_soft: Color32::from_rgb(248, 225, 219),
            text: Color32::from_rgb(32, 39, 50),
            muted: Color32::from_rgb(105, 116, 132),
            accent: Color32::from_rgb(57, 120, 220),
            conflict_fill: Color32::from_rgb(255, 224, 216),
            active_conflict_fill: Color32::from_rgb(255, 194, 178),
            conflict_text: Color32::from_rgb(154, 52, 42),
            added_fill: Color32::from_rgb(215, 246, 224),
            added_text: Color32::from_rgb(32, 128, 72),
            base_only_fill: Color32::from_rgb(214, 221, 230),
            base_only_connector_fill: Color32::from_rgb(214, 221, 230),
            base_only_text: Color32::from_rgb(92, 102, 116),
            connector: Color32::from_rgb(92, 145, 98),
            result_fill: Color32::from_rgb(255, 255, 255),
            shadow: eframe::epaint::Shadow {
                offset: [3, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_rgba_unmultiplied(44, 56, 72, 44),
            },
        },
    }
}

pub fn merge_theme_label(language: MergeLanguage, theme: MergeTheme) -> &'static str {
    match theme {
        MergeTheme::Dark => mt(language, "dark"),
        MergeTheme::Light => mt(language, "light"),
    }
}

pub fn merge_language_label(language: MergeLanguage) -> &'static str {
    match language {
        MergeLanguage::Chinese => "中文",
        MergeLanguage::English => "EN",
    }
}

fn merge_ignore_mode_label(language: MergeLanguage, mode: MergeIgnoreMode) -> &'static str {
    match (language, mode) {
        (MergeLanguage::Chinese, MergeIgnoreMode::None) => "不忽略",
        (MergeLanguage::Chinese, MergeIgnoreMode::TrimWhitespace) => "忽略首尾空白",
        (MergeLanguage::Chinese, MergeIgnoreMode::IgnoreWhitespace) => "忽略全部空白",
        (_, MergeIgnoreMode::None) => "Do not ignore",
        (_, MergeIgnoreMode::TrimWhitespace) => "Trim whitespaces",
        (_, MergeIgnoreMode::IgnoreWhitespace) => "Ignore whitespaces",
    }
}

fn merge_highlight_mode_label(language: MergeLanguage, mode: MergeHighlightMode) -> &'static str {
    match (language, mode) {
        (MergeLanguage::Chinese, MergeHighlightMode::Lines) => "高亮行",
        (MergeLanguage::Chinese, MergeHighlightMode::Words) => "高亮词",
        (_, MergeHighlightMode::Lines) => "Highlight lines",
        (_, MergeHighlightMode::Words) => "Highlight words",
    }
}

fn mt(language: MergeLanguage, key: &str) -> &'static str {
    match (language, key) {
        (MergeLanguage::Chinese, "title") => "合并修订",
        (MergeLanguage::Chinese, "conflicts") => "个冲突",
        (MergeLanguage::Chinese, "auto_applied") => "非冲突内容已自动合并",
        (MergeLanguage::Chinese, "no_changes") => "无其他变更。",
        (MergeLanguage::Chinese, "conflict_count") => "个冲突。",
        (MergeLanguage::Chinese, "local") => "本地变更",
        (MergeLanguage::Chinese, "remote") => "远端变更",
        (MergeLanguage::Chinese, "result") => "合并结果",
        (MergeLanguage::Chinese, "accept_left") => "使用我的版本",
        (MergeLanguage::Chinese, "accept_right") => "使用他的版本",
        (MergeLanguage::Chinese, "apply") => "应用",
        (MergeLanguage::Chinese, "cancel") => "取消",
        (MergeLanguage::Chinese, "light") => "白天",
        (MergeLanguage::Chinese, "dark") => "黑夜",
        (MergeLanguage::Chinese, "applying") => "应用中...",
        (MergeLanguage::Chinese, "write_failed") => "写入失败",
        (MergeLanguage::Chinese, "write_stopped") => "写入已停止",
        (MergeLanguage::Chinese, "resolve_all_conflicts") => {
            "\u{8bf7}\u{5148}\u{89e3}\u{51b3}\u{6240}\u{6709}\u{51b2}\u{7a81}"
        }
        (MergeLanguage::Chinese, "result_placeholder") => {
            "\u{8bf7}\u{8f93}\u{5165}\u{5408}\u{5e76}\u{7ed3}\u{679c}"
        }
        (MergeLanguage::Chinese, "cancel_merge_title") => "\u{53d6}\u{6d88}\u{5408}\u{5e76}",
        (MergeLanguage::Chinese, "cancel_merge_message") => {
            "\u{5408}\u{5e76}\u{7ed3}\u{679c}\u{4e2d}\u{6709}\u{672a}\u{4fdd}\u{5b58}\u{7684}\u{66f4}\u{6539}\u{3002}\u{8981}\u{4e22}\u{5f03}\u{66f4}\u{6539}\u{5e76}\u{53d6}\u{6d88}\u{5408}\u{5e76}\u{5417}\u{ff1f}"
        }
        (MergeLanguage::Chinese, "cancel_merge_discard") => {
            "\u{4e22}\u{5f03}\u{66f4}\u{6539}\u{5e76}\u{53d6}\u{6d88}\u{5408}\u{5e76}"
        }
        (MergeLanguage::Chinese, "cancel_merge_continue") => "\u{7ee7}\u{7eed}\u{5408}\u{5e76}",
        (MergeLanguage::Chinese, "edit_result") => "\u{7f16}\u{8f91}\u{7ed3}\u{679c}",
        (MergeLanguage::Chinese, "editing_result") => "\u{6b63}\u{5728}\u{7f16}\u{8f91}",
        (MergeLanguage::Chinese, "collapse_unchanged") => "收起未变块",
        (MergeLanguage::Chinese, "expand_unchanged") => "展开未变块",
        (MergeLanguage::Chinese, "unchanged_lines") => "行未变更",
        (MergeLanguage::Chinese, "manual_result_hint") => {
            "\u{624b}\u{52a8}\u{7f16}\u{8f91}\u{540e}\u{5c06}\u{4ee5}\u{4e2d}\u{95f4}\u{7ed3}\u{679c}\u{4e3a}\u{51c6}"
        }
        (_, "title") => "Merge Revisions",
        (_, "conflicts") => "conflict(s)",
        (_, "auto_applied") => "Non-conflicting changes auto-applied",
        (_, "no_changes") => "No changes.",
        (_, "conflict_count") => "conflict(s).",
        (_, "local") => "Local Changes",
        (_, "remote") => "Remote Changes",
        (_, "result") => "Result",
        (_, "accept_left") => "Use Mine",
        (_, "accept_right") => "Use Theirs",
        (_, "apply") => "Apply",
        (_, "cancel") => "Cancel",
        (_, "light") => "Light",
        (_, "dark") => "Dark",
        (_, "applying") => "Applying...",
        (_, "write_failed") => "Failed to write",
        (_, "write_stopped") => "Write stopped",
        (_, "resolve_all_conflicts") => "Resolve all conflicts before applying",
        (_, "result_placeholder") => "Enter merge result",
        (_, "cancel_merge_title") => "Cancel Merge",
        (_, "cancel_merge_message") => {
            "There are unsaved changes in the result file. Discard changes and cancel merge anyway?"
        }
        (_, "cancel_merge_discard") => "Discard Changes and Cancel Merge",
        (_, "cancel_merge_continue") => "Continue Merge",
        (_, "edit_result") => "Edit Result",
        (_, "editing_result") => "Editing",
        (_, "collapse_unchanged") => "Collapse unchanged",
        (_, "expand_unchanged") => "Expand unchanged",
        (_, "unchanged_lines") => "unchanged lines",
        (_, "manual_result_hint") => {
            "Manual edits resolve the remaining conflicts from this result."
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_merge_args() -> MergeArgs {
        MergeArgs {
            base: PathBuf::from("base.txt"),
            local: PathBuf::from("local.txt"),
            remote: PathBuf::from("remote.txt"),
            output: PathBuf::from("merged.txt"),
            repo_root: None,
            stage: false,
            theme: MergeTheme::Light,
            language: MergeLanguage::Chinese,
        }
    }

    #[test]
    fn word_highlight_marks_only_changed_tokens_in_order() {
        let text = "runtime_mode = local";
        let reference = "runtime_mode = base";
        let ranges = merge_word_highlight_ranges(text, reference);

        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "local");
    }

    #[test]
    fn word_highlight_does_not_confuse_repeated_tokens() {
        let text = "alpha beta alpha";
        let reference = "alpha gamma alpha";
        let ranges = merge_word_highlight_ranges(text, reference);

        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "beta");
    }

    #[test]
    fn word_highlight_marks_a_pure_insertion_as_all_new() {
        let text = "local_insert_01 = enabled";
        let ranges = merge_word_highlight_ranges(text, "");

        assert_eq!(ranges.len(), 3);
        assert_eq!(&text[ranges[0].clone()], "local_insert_01");
        assert_eq!(&text[ranges[1].clone()], "=");
        assert_eq!(&text[ranges[2].clone()], "enabled");
    }

    #[test]
    fn word_mode_preserves_a_shallow_changed_line_background() {
        let palette = merge_palette(MergeTheme::Light);
        assert_ne!(
            merge_code_row_fill(
                MergeSideLineTone::Replaced,
                true,
                false,
                MergeHighlightMode::Words,
                palette,
            ),
            None
        );
    }

    #[test]
    fn word_mode_marks_non_conflicting_insertions_with_a_shallow_line_background() {
        let palette = merge_palette(MergeTheme::Light);

        assert_ne!(
            merge_code_row_fill(
                MergeSideLineTone::Added,
                false,
                false,
                MergeHighlightMode::Words,
                palette,
            ),
            None
        );
        assert_eq!(
            merge_code_row_fill(
                MergeSideLineTone::Added,
                false,
                false,
                MergeHighlightMode::Lines,
                palette,
            ),
            None
        );
    }

    #[test]
    fn word_mode_marks_unresolved_insertions_with_the_conflict_background() {
        let palette = merge_palette(MergeTheme::Light);
        let unresolved = merge_code_row_fill(
            MergeSideLineTone::Added,
            true,
            false,
            MergeHighlightMode::Words,
            palette,
        );
        let non_conflicting = merge_code_row_fill(
            MergeSideLineTone::Added,
            false,
            false,
            MergeHighlightMode::Words,
            palette,
        );

        assert_ne!(unresolved, None);
        assert_ne!(unresolved, non_conflicting);
        assert_eq!(
            unresolved.map(|color| color.a()),
            Some((255.0 * MERGE_WORD_BLOCK_OPACITY).round() as u8)
        );
    }

    #[test]
    fn result_editor_has_localized_placeholder() {
        let source = include_str!("merge_tool.rs");

        assert!(source.contains("mt(app.language, \"result_placeholder\")"));
        assert!(source.contains("TextEdit::singleline(text)"));
        assert!(source.contains("merge_editable_result_row"));
        assert!(source.contains("manual_result_override"));
        assert_eq!(
            mt(MergeLanguage::Chinese, "result_placeholder"),
            "\u{8bf7}\u{8f93}\u{5165}\u{5408}\u{5e76}\u{7ed3}\u{679c}"
        );
        assert_eq!(
            mt(MergeLanguage::English, "result_placeholder"),
            "Enter merge result"
        );
    }

    #[test]
    fn manual_result_edit_resolves_conflicts_and_is_undoable() {
        let document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);

        assert_eq!(app.unresolved_conflict_count(), 1);
        assert!(!app.can_apply_result());

        let before = app.snapshot();
        app.manual_result_lines = vec![
            "keep".to_owned(),
            "manual result".to_owned(),
            "end".to_owned(),
        ];
        app.finish_manual_result_edit(before);

        assert!(app.manual_result_override);
        assert_eq!(app.unresolved_conflict_count(), 0);
        assert!(app.can_apply_result());
        assert_eq!(app.result_text, "keep\nmanual result\nend\n");

        assert!(app.undo());
        assert!(!app.manual_result_override);
        assert_eq!(app.unresolved_conflict_count(), 1);
        assert!(!app.can_apply_result());

        assert!(app.redo());
        assert!(app.manual_result_override);
        assert_eq!(app.result_text, "keep\nmanual result\nend\n");
    }

    #[test]
    fn large_merge_uses_cached_virtual_rows_with_visible_connectors() {
        let content = (0..MERGE_VIRTUAL_ROW_THRESHOLD)
            .map(|index| format!("lock-entry-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let document = three_way_merge(&content, &content, &content);
        let app = MergeToolApp::new(test_merge_args(), document);
        let source = include_str!("merge_tool.rs");
        let columns_source = source
            .split("fn merge_editor_columns")
            .nth(1)
            .and_then(|tail| tail.split("fn merge_side_panel").next())
            .expect("merge editor columns implementation");
        let side_panel_source = source
            .split("fn merge_side_panel")
            .nth(1)
            .and_then(|tail| tail.split("fn merge_result_panel").next())
            .expect("merge side panel implementation");

        assert!(app.uses_virtual_merge_rows());
        assert_eq!(app.local_display_rows.len(), MERGE_VIRTUAL_ROW_THRESHOLD);
        assert_eq!(app.remote_display_rows.len(), MERGE_VIRTUAL_ROW_THRESHOLD);
        assert!(source.contains(".show_rows(ui, MERGE_CODE_ROW_HEIGHT, display_rows"));
        assert!(!columns_source.contains("if !use_virtual_rows"));
        assert!(!side_panel_source.contains("if !use_virtual_rows"));
        assert!(columns_source.contains("paint_merge_block_connectors("));
        assert!(side_panel_source.contains("paint_base_only_side_overlays("));
    }

    #[test]
    fn virtual_visible_geometry_keeps_conflict_and_base_only_connectors() {
        let prefix = (0..MERGE_VIRTUAL_ROW_THRESHOLD)
            .map(|index| format!("lock-entry-{index}"))
            .collect::<Vec<_>>();

        let mut conflict_base = prefix.clone();
        conflict_base.extend(["base conflict".to_owned(), "end".to_owned()]);
        let mut conflict_local = prefix.clone();
        conflict_local.extend(["local conflict".to_owned(), "end".to_owned()]);
        let mut conflict_remote = prefix.clone();
        conflict_remote.extend(["remote conflict".to_owned(), "end".to_owned()]);
        let conflict_document = three_way_merge(
            &(conflict_base.join("\n") + "\n"),
            &(conflict_local.join("\n") + "\n"),
            &(conflict_remote.join("\n") + "\n"),
        );
        let conflict = &conflict_document.conflicts()[0];
        let mut result_geometry = MergePanelGeometry::default();
        let (result_first, result_count) =
            merge_result_row_span_for_conflict(&conflict_document, conflict).unwrap();
        for offset in 0..result_count {
            result_geometry.record_row(
                result_first + offset,
                Rect::from_min_size(
                    Pos2::new(400.0, 100.0 + offset as f32 * MERGE_CODE_ROW_HEIGHT),
                    Vec2::new(300.0, MERGE_CODE_ROW_HEIGHT),
                ),
            );
        }
        assert!(
            merge_block_result_rect_from_geometry(&conflict_document, conflict, &result_geometry)
                .is_some()
        );
        for side in [MergeSide::Local, MergeSide::Remote] {
            let (side_first, side_count) =
                merge_side_row_span_for_conflict(&conflict_document, side, conflict).unwrap();
            let mut side_geometry = MergePanelGeometry::default();
            for offset in 0..side_count {
                side_geometry.record_row(
                    side_first + offset,
                    Rect::from_min_size(
                        Pos2::new(20.0, 100.0 + offset as f32 * MERGE_CODE_ROW_HEIGHT),
                        Vec2::new(300.0, MERGE_CODE_ROW_HEIGHT),
                    ),
                );
            }
            assert!(
                merge_block_side_rect_from_geometry(
                    &conflict_document,
                    conflict,
                    side,
                    &side_geometry
                )
                .is_some()
            );
        }

        let mut deletion_base = prefix.clone();
        deletion_base.extend([
            "keep".to_owned(),
            ".claude/*".to_owned(),
            ".agents/*".to_owned(),
            "end".to_owned(),
        ]);
        let mut deletion_local = prefix.clone();
        deletion_local.extend(["keep".to_owned(), "end".to_owned()]);
        let deletion_document = three_way_merge(
            &(deletion_base.join("\n") + "\n"),
            &(deletion_local.join("\n") + "\n"),
            &(deletion_base.join("\n") + "\n"),
        );
        let group = base_only_display_groups(&deletion_document)
            .into_iter()
            .next()
            .expect("large base-only deletion group");
        let result_first =
            merge_result_display_row_for_line(&deletion_document, group.line_index).unwrap();
        let mut deletion_result_geometry = MergePanelGeometry::default();
        for offset in 0..group.line_count {
            deletion_result_geometry.record_row(
                result_first + offset,
                Rect::from_min_size(
                    Pos2::new(400.0, 200.0 + offset as f32 * MERGE_CODE_ROW_HEIGHT),
                    Vec2::new(300.0, MERGE_CODE_ROW_HEIGHT),
                ),
            );
        }
        assert!(
            merge_base_only_result_rect_from_geometry(
                &deletion_document,
                group,
                &deletion_result_geometry
            )
            .is_some()
        );

        let boundary = merge_side_display_row_for_line(
            &deletion_document,
            group.missing_side,
            group.line_index,
        )
        .unwrap();
        let mut deletion_side_geometry = MergePanelGeometry::default();
        deletion_side_geometry.record_row(
            boundary,
            Rect::from_min_size(
                Pos2::new(20.0, 200.0),
                Vec2::new(300.0, MERGE_CODE_ROW_HEIGHT),
            ),
        );
        assert!(
            merge_base_only_side_rect_from_geometry(
                &deletion_document,
                group,
                &deletion_side_geometry
            )
            .is_some()
        );
    }

    #[test]
    fn large_merge_cached_scroll_anchors_keep_conflict_rows_aligned() {
        let mut base = (0..6_000)
            .map(|index| format!("lock-entry-{index}: base"))
            .collect::<Vec<_>>();
        let mut local = base.clone();
        let mut remote = base.clone();
        local.splice(
            1_000..1_000,
            (0..240).map(|index| format!("local-only-{index}: true")),
        );
        base[5_500] = "lock-entry-5500: base".to_owned();
        local[5_740] = "lock-entry-5500: local".to_owned();
        remote[5_500] = "lock-entry-5500: remote".to_owned();
        let document = three_way_merge(
            &(base.join("\n") + "\n"),
            &(local.join("\n") + "\n"),
            &(remote.join("\n") + "\n"),
        );
        assert_eq!(document.unresolved_conflict_count(), 1);
        let conflict_index = document.conflicts()[0].index;
        let app = MergeToolApp::new(test_merge_args(), document);
        assert!(app.uses_virtual_merge_rows());

        let result_scroll =
            merge_result_scroll_y_for_conflict(&app.document, conflict_index).unwrap();
        for side in [MergeSide::Local, MergeSide::Remote] {
            let expected_side_row = match side {
                MergeSide::Local => &app.local_display_rows,
                MergeSide::Remote => &app.remote_display_rows,
            }
            .iter()
            .position(|row| row.conflict_index == Some(conflict_index))
            .unwrap() as f32;
            let mapped_side_row = app.cached_side_scroll_y_for_result_scroll(side, result_scroll)
                / MERGE_CODE_ROW_HEIGHT;
            assert!(
                (mapped_side_row - expected_side_row).abs() < 0.01,
                "{side:?} conflict mapped to {mapped_side_row}, expected {expected_side_row}"
            );

            let mapped_result_row = app.cached_result_scroll_y_for_side_scroll(
                side,
                expected_side_row * MERGE_CODE_ROW_HEIGHT,
            ) / MERGE_CODE_ROW_HEIGHT;
            assert!(
                (mapped_result_row - result_scroll / MERGE_CODE_ROW_HEIGHT).abs() < 0.01,
                "{side:?} reverse mapping returned {mapped_result_row}"
            );
        }
    }

    #[test]
    fn large_merge_cached_scroll_anchors_align_remote_deletion_gap() {
        let base = (0..8_000)
            .map(|index| format!("package-{index:05}: base"))
            .collect::<Vec<_>>();
        let local = base.clone();
        let mut remote = base.clone();
        remote.drain(6_375..6_379);
        let document = three_way_merge(
            &(base.join("\n") + "\n"),
            &(local.join("\n") + "\n"),
            &(remote.join("\n") + "\n"),
        );
        let group = base_only_display_groups(&document)
            .into_iter()
            .find(|group| group.missing_side == MergeSide::Remote)
            .expect("remote deletion group");
        assert_eq!(group.line_count, 4);

        let result_first = merge_result_display_row_for_line(&document, group.line_index).unwrap();
        let remote_boundary =
            merge_side_display_row_for_line(&document, MergeSide::Remote, group.line_index)
                .unwrap();
        let app = MergeToolApp::new(test_merge_args(), document);
        assert!(app.uses_virtual_merge_rows());

        for result_row in [result_first, result_first + group.line_count] {
            let mapped_remote_row = app.cached_side_scroll_y_for_result_scroll(
                MergeSide::Remote,
                result_row as f32 * MERGE_CODE_ROW_HEIGHT,
            ) / MERGE_CODE_ROW_HEIGHT;
            assert!(
                (mapped_remote_row - remote_boundary as f32).abs() < 0.01,
                "result row {result_row} mapped to {mapped_remote_row}, expected deletion boundary {remote_boundary}"
            );
        }

        let rows_above = 18;
        let result_scroll_row = result_first - rows_above;
        let mapped_remote_scroll_row = app.cached_side_scroll_y_for_result_scroll(
            MergeSide::Remote,
            result_scroll_row as f32 * MERGE_CODE_ROW_HEIGHT,
        ) / MERGE_CODE_ROW_HEIGHT;
        let result_screen_row = result_first as f32 - result_scroll_row as f32;
        let remote_screen_row = remote_boundary as f32 - mapped_remote_scroll_row;
        assert!(
            (result_screen_row - remote_screen_row).abs() < 0.01,
            "result deletion appears at screen row {result_screen_row}, remote gap at {remote_screen_row}"
        );
    }

    #[test]
    fn conflict_navigation_wraps_and_reselects_a_single_conflict() {
        let document = three_way_merge("base\n", "local\n", "remote\n");
        assert_eq!(document.unresolved_conflict_count(), 1);
        let mut cursor = document.conflicts()[0].index;

        previous_unresolved_conflict(&document, MergeSide::Local, &mut cursor);
        assert_eq!(cursor, document.conflicts()[0].index);
        next_unresolved_conflict(&document, MergeSide::Local, &mut cursor);
        assert_eq!(cursor, document.conflicts()[0].index);
    }

    #[test]
    fn conflict_navigation_wraps_between_multiple_conflicts() {
        let document = three_way_merge(
            "alpha\nstable\nbeta\n",
            "alpha-local\nstable\nbeta-local\n",
            "alpha-remote\nstable\nbeta-remote\n",
        );
        assert_eq!(document.unresolved_conflict_count(), 2);
        let first = document.conflicts()[0].index;
        let last = document.conflicts()[1].index;
        let mut cursor = first;

        previous_unresolved_conflict(&document, MergeSide::Local, &mut cursor);
        assert_eq!(cursor, last);
        next_unresolved_conflict(&document, MergeSide::Local, &mut cursor);
        assert_eq!(cursor, first);
    }

    #[test]
    fn deleting_a_base_line_and_adding_after_it_on_the_other_side_auto_merges() {
        let document = three_way_merge(
            "base\nalpha change\n",
            "base\n",
            "base\nalpha change\nbeta change\n",
        );

        assert_eq!(document.unresolved_conflict_count(), 0);
        assert_eq!(document.result_text(), "base\nbeta change\n");
    }

    #[test]
    fn whitespace_ignore_modes_change_three_way_comparison_without_normalizing_output() {
        let document = three_way_merge_with_options(
            "value = 1\n",
            "value=1\n",
            "value = 1\n",
            MergeIgnoreMode::IgnoreWhitespace,
        );

        assert_eq!(document.unresolved_conflict_count(), 0);
        assert_eq!(document.result_text(), "value = 1\n");
        assert_eq!(
            three_way_merge("value = 1\n", "value=1\n", "value = 1\n").result_text(),
            "value=1\n"
        );
    }

    #[test]
    fn collapsed_tail_keeps_changed_rows_and_small_context() {
        let rows = (0..20)
            .map(|index| CachedMergeSideDisplayRow {
                text: format!("line-{index}"),
                reference_text: None,
                line_number: Some(index + 1),
                conflict_index: (index == 2).then_some(0),
                side_resolved: false,
                tone: MergeSideLineTone::Unchanged,
                show_conflict_actions: false,
                action_target: None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            merge_visible_tail_len(&rows, true),
            3 + MERGE_COLLAPSE_CONTEXT_ROWS
        );
        assert_eq!(merge_visible_tail_len(&rows, false), rows.len());
    }

    #[test]
    fn collapsed_overview_matches_the_editor_row_count() {
        let rows = (0..20)
            .map(|index| CachedMergeSideDisplayRow {
                text: format!("line-{index}"),
                reference_text: None,
                line_number: Some(index + 1),
                conflict_index: (index == 2).then_some(0),
                side_resolved: false,
                tone: MergeSideLineTone::Unchanged,
                show_conflict_actions: false,
                action_target: None,
            })
            .collect::<Vec<_>>();

        let visible = merge_visible_tail_len(&rows, true);
        let overview = merge_visible_side_overview_tones(&rows, true);

        assert_eq!(overview.len(), visible + 1);
        assert_eq!(overview.last(), Some(&MergeSideLineTone::Unchanged));
    }

    #[test]
    fn hidden_scroll_area_offsets_are_clamped_before_sharing() {
        assert_eq!(merge_clamp_scroll_offset(480.0, 180.0, 120.0), 60.0);
        assert_eq!(merge_clamp_scroll_offset(-12.0, 180.0, 120.0), 0.0);
        assert_eq!(merge_clamp_scroll_offset(42.0, 80.0, 120.0), 0.0);
    }

    #[test]
    fn merge_toolbar_exposes_ignore_highlight_collapse_and_minimap_controls() {
        let source = include_str!("merge_tool.rs");

        assert!(source.contains("MergeIgnoreMode::IgnoreWhitespace"));
        assert!(source.contains("MergeHighlightMode::Words"));
        assert!(source.contains("collapse_unchanged"));
        assert!(source.contains("merge_overview_target"));
        assert!(source.contains("ScrollBarVisibility::AlwaysHidden"));
    }

    #[test]
    fn merge_tool_actions_are_undoable_and_redoable() {
        let document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);

        assert!(!app.has_unsaved_edits());
        assert!(!app.can_undo());
        assert!(!app.can_redo());

        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Local,
            MergeLineAction::Take,
        );

        assert!(app.has_unsaved_edits());
        assert!(app.can_undo());
        assert_eq!(app.result_text, "keep\nlocal\nend\n");

        assert!(app.undo());
        assert_eq!(app.result_text, "keep\nend\n");
        assert!(!app.has_unsaved_edits());
        assert!(app.can_redo());

        assert!(app.redo());
        assert_eq!(app.result_text, "keep\nlocal\nend\n");
        assert!(app.has_unsaved_edits());
    }

    #[test]
    fn new_merge_action_clears_redo_history() {
        let document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);

        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Local,
            MergeLineAction::Take,
        );
        assert!(app.undo());
        assert!(app.can_redo());

        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Remote,
            MergeLineAction::Take,
        );

        assert_eq!(app.result_text, "keep\nremote\nend\n");
        assert!(!app.can_redo());
    }

    #[test]
    fn conflict_is_resolved_only_after_both_side_decisions() {
        let mut document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );

        assert_eq!(document.unresolved_conflict_count(), 1);
        document.drop_conflict_side(0, MergeSide::Local);
        assert_eq!(document.unresolved_conflict_count(), 1);
        document.take_conflict_side(0, MergeSide::Remote);

        assert_eq!(document.unresolved_conflict_count(), 0);
        assert_eq!(document.result_text(), "keep\nremote\nend\n");
    }

    #[test]
    fn resolved_empty_conflict_does_not_fall_back_to_base_preview() {
        let mut document =
            three_way_merge("keep\nbase\nend\n", "keep\nlocal\nend\n", "keep\nend\n");

        document.drop_conflict_side(0, MergeSide::Local);
        document.take_conflict_side(0, MergeSide::Remote);

        assert_eq!(document.unresolved_conflict_count(), 0);
        assert_eq!(document.result_text(), "keep\nend\n");
        assert!(!merge_result_display_lines(&document).contains(&"base"));
        assert!(!merge_result_display_lines(&document).contains(&"local"));
    }

    #[test]
    fn apply_stays_disabled_until_every_conflict_is_resolved() {
        let document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);

        assert!(!app.can_apply_result());
        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Local,
            MergeLineAction::Drop,
        );
        assert!(!app.can_apply_result());
        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Remote,
            MergeLineAction::Take,
        );
        assert!(app.can_apply_result());
    }

    #[test]
    fn taking_remote_after_dropping_local_handles_moved_remote_block() {
        let base = "shell\ntrust\nminimum\nmicro\nversioned-ant\n";
        let local = "shell\ntrust\nminimum\nmicro\n";
        let remote = "minimum\nmicro\nunversioned-ant\nshell\ntrust\n";
        let mut document = three_way_merge(base, local, remote);

        assert_eq!(document.conflicts().len(), 1);
        document.drop_conflict_side(0, MergeSide::Local);
        document.take_conflict_side(0, MergeSide::Remote);

        assert_eq!(document.unresolved_conflict_count(), 0);
        assert_eq!(document.result_text(), remote);
        assert_eq!(
            merge_result_display_lines(&document).join("\n") + "\n",
            remote
        );
    }

    #[test]
    fn cancel_merge_prompts_only_after_user_edits() {
        let document = three_way_merge(
            "keep\nbase\nend\n",
            "keep\nlocal\nend\n",
            "keep\nremote\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);

        assert_eq!(app.request_cancel(), MergeCancelRequest::ExitNow);
        assert!(!app.show_cancel_confirm);

        app.apply_line_action(
            MergeLineActionTarget::Conflict(0),
            MergeSide::Local,
            MergeLineAction::Take,
        );

        assert_eq!(app.request_cancel(), MergeCancelRequest::ShowConfirm);
        assert!(app.show_cancel_confirm);
    }

    #[test]
    fn cancel_merge_confirmation_is_localized_and_close_is_intercepted() {
        let source = include_str!("merge_tool.rs");

        assert_eq!(MERGE_TOOL_CANCEL_EXIT_CODE, 10);
        assert!(source.contains("std::process::exit(MERGE_TOOL_CANCEL_EXIT_CODE)"));
        assert_eq!(
            mt(MergeLanguage::English, "cancel_merge_title"),
            "Cancel Merge"
        );
        assert_eq!(
            mt(MergeLanguage::Chinese, "cancel_merge_title"),
            "\u{53d6}\u{6d88}\u{5408}\u{5e76}"
        );
        assert!(source.contains("ViewportCommand::CancelClose"));
        assert!(source.contains("viewport().close_requested()"));
        assert!(source.contains("ctrl && i.key_pressed(egui::Key::Z)"));
        assert!(source.contains("ctrl && i.key_pressed(egui::Key::Y)"));
    }

    #[test]
    fn consecutive_conflict_lines_form_one_block() {
        let document = three_way_merge(
            "keep\nbase-a\nbase-b\nbase-c\n",
            "keep\nlocal-a\nlocal-b\n",
            "keep\nremote-a\nremote-b\nremote-c\nremote-d\n",
        );

        assert_eq!(document.conflicts().len(), 1);
        let conflict = &document.conflicts()[0];
        assert_eq!(conflict.local, vec!["local-a", "local-b"]);
        assert_eq!(
            conflict.remote,
            vec!["remote-a", "remote-b", "remote-c", "remote-d"]
        );
        assert_eq!(conflict.line_indices.len(), 4);
        assert!(
            conflict
                .line_indices
                .iter()
                .all(|index| document.lines[*index].conflict_index == Some(0))
        );
    }

    #[test]
    fn unresolved_conflict_result_does_not_include_base_lines() {
        let document = three_way_merge(
            "node_modules/\nbuild/\ncache/\ngraph-cost.json\n",
            "node_modules/\ndist-local/\ncache-local/\ngraph-cost.json\n",
            "node_modules/\nrelease-dist/\ngraph-cache/\ngraph-cost.json\n",
        );

        assert!(document.result_text().contains("node_modules/"));
        assert!(document.result_text().contains("graph-cost.json"));
        assert!(!document.result_text().contains("build/"));
        assert!(!document.result_text().contains("cache/"));
        assert!(
            document
                .lines
                .iter()
                .filter(|line| line.kind == MergeLineKind::Conflict)
                .all(|line| !line.include_in_result)
        );
    }

    #[test]
    fn insertion_conflict_uses_a_boundary_marker_without_a_blank_result_row() {
        let document = three_way_merge(
            "before\nafter\n",
            "before\nlocal add\nafter\n",
            "before\nremote add\nafter\n",
        );
        let conflict = document
            .conflicts()
            .iter()
            .find(|conflict| conflict.base.is_empty())
            .expect("insertion conflict");

        assert!(merge_result_row_span_for_conflict(&document, conflict).is_none());
        assert_eq!(
            merge_result_display_lines(&document),
            vec!["before", "after"]
        );
        assert_eq!(
            merge_result_display_boundary_before_line(
                &document,
                *conflict.line_indices.first().unwrap(),
            ),
            1,
        );
    }

    #[test]
    fn word_mode_paints_every_row_of_an_unresolved_insertion_conflict() {
        let document = three_way_merge(
            "before\nafter\n",
            "before\nlocal insert one\nlocal insert two\nafter\n",
            "before\nremote insert one\nremote insert two\nafter\n",
        );
        let palette = merge_palette(MergeTheme::Light);
        let rows = merge_side_display_rows(&document, MergeSide::Local);
        let conflict_rows = rows
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .collect::<Vec<_>>();

        assert_eq!(conflict_rows.len(), 2);
        assert!(conflict_rows.iter().all(|row| {
            row.tone == MergeSideLineTone::Replaced
                && merge_code_row_fill(
                    row.tone,
                    !row.side_resolved,
                    false,
                    MergeHighlightMode::Words,
                    palette,
                )
                .is_some_and(|fill| fill.a() >= 200)
        }));

        let cached_rows = cached_merge_side_display_rows(&document, MergeSide::Local);
        let first_conflict_row = cached_rows
            .iter()
            .position(|row| row.conflict_index == Some(0))
            .expect("insertion conflict row");
        assert_eq!(
            merge_side_background_run(
                &cached_rows,
                first_conflict_row,
                0,
                cached_rows.len(),
                usize::MAX,
                MergeHighlightMode::Words,
                palette,
            ),
            Some((palette.conflict_fill, 2))
        );
    }

    #[test]
    fn result_display_rows_show_unresolved_base_blocks_without_side_lines() {
        let document = three_way_merge(
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\nbuild/\ncache/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\n.vscode\nvite.config.mts.*.mjs\nnil\nCLAUDE.md\n.codegraph/*\nAGENTS.md\n.codex/*\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\ndist-local/\ncache-local/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nCLAUDE.local.md\n.codegraph-local/*\nAGENTS.local.md\n.codex-local/*\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\nrelease-dist/\ngraph-cache/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\n**/graphify-out/cache/\n**/graphify-out/cost.json\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
        );

        let rows = merge_result_display_lines(&document);

        assert!(rows.contains(&"build/"));
        assert!(rows.contains(&"cache/"));
        assert!(!rows.contains(&"dist-local/"));
        assert!(!rows.contains(&"release-dist/"));
        assert_eq!(rows.iter().filter(|row| row.is_empty()).count(), 4);
        assert_eq!(rows[8], "node_modules/");
        assert_eq!(rows[9], "build/");
        assert_eq!(rows[10], "cache/");
        assert!(rows.contains(&".claude/*"));
        assert!(rows.contains(&".agents/*"));
        assert_eq!(rows[22], "nil");
        assert_eq!(rows.len(), 31);
    }

    #[test]
    fn result_display_rows_show_unresolved_base_replacement_rows() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );

        let rows = merge_result_display_lines(&document);

        assert_eq!(rows, vec!["keep", "build/", "cache/", "end"]);
    }

    #[test]
    fn result_connector_rect_spans_unresolved_base_replacement_rows() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );
        let panel = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(360.0, 640.0));
        let rect = merge_block_result_rect(panel, &document, &document.conflicts()[0], 0.0)
            .expect("replacement result block");
        let top = merge_scroll_content_top(panel) + MERGE_CODE_ROW_HEIGHT;

        assert_eq!(rect.top(), top);
        assert_eq!(rect.bottom(), top + MERGE_CODE_ROW_HEIGHT * 2.0);
    }

    #[test]
    fn result_connector_rect_extends_to_line_number_gutter() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );
        let panel = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(360.0, 640.0));
        let rect = merge_block_result_rect(panel, &document, &document.conflicts()[0], 0.0)
            .expect("result connector rect");

        assert_eq!(rect.left(), merge_scroll_clip_rect(panel).left());
    }

    #[test]
    fn base_only_result_connector_extends_to_line_number_gutter() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let panel = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(360.0, 640.0));
        let group = base_only_display_groups(&document)
            .into_iter()
            .next()
            .expect("base-only group");
        let rect = merge_base_only_result_rect(panel, &document, group, 0.0)
            .expect("base-only result connector rect");

        assert_eq!(rect.left(), merge_scroll_clip_rect(panel).left());
    }

    #[test]
    fn base_only_rows_do_not_draw_top_or_bottom_outline() {
        assert!(!should_paint_result_block_outline(
            MergeSideLineTone::BaseOnly
        ));
        assert!(should_paint_result_block_outline(MergeSideLineTone::Added));
        assert!(!should_paint_result_block_outline(
            MergeSideLineTone::Replaced
        ));
    }

    #[test]
    fn connector_fill_uses_row_color_with_ninety_percent_opacity() {
        let palette = merge_palette(MergeTheme::Light);

        assert_eq!(
            merge_connector_fill(MergeSideLineTone::BaseOnly, palette),
            Color32::from_rgba_unmultiplied(214, 221, 230, 230)
        );

        assert_eq!(
            merge_connector_fill(MergeSideLineTone::Replaced, palette),
            Color32::from_rgba_unmultiplied(
                palette.conflict_fill.r(),
                palette.conflict_fill.g(),
                palette.conflict_fill.b(),
                230,
            )
        );
    }

    #[test]
    fn active_conflict_fill_is_distinct_from_regular_conflict_fill() {
        for theme in [MergeTheme::Light, MergeTheme::Dark] {
            let palette = merge_palette(theme);
            assert_ne!(palette.active_conflict_fill, palette.conflict_fill);
        }
    }

    #[test]
    fn word_mode_keeps_base_only_deletions_fully_visible() {
        let palette = merge_palette(MergeTheme::Light);

        assert_eq!(
            merge_highlight_fill(
                MergeSideLineTone::BaseOnly,
                palette.base_only_fill,
                false,
                MergeHighlightMode::Words,
            ),
            palette.base_only_fill
        );
        assert_eq!(
            merge_highlight_fill(
                MergeSideLineTone::Replaced,
                palette.conflict_fill,
                false,
                MergeHighlightMode::Words,
            ),
            palette.conflict_fill
        );
    }

    #[test]
    fn contiguous_result_conflict_rows_use_one_background_span() {
        let palette = merge_palette(MergeTheme::Light);
        let styles = vec![
            (MergeSideLineTone::Replaced, false),
            (MergeSideLineTone::Replaced, false),
            (MergeSideLineTone::Unchanged, false),
        ];

        assert_eq!(
            merge_result_background_run(
                &styles,
                0,
                0,
                styles.len(),
                MergeHighlightMode::Lines,
                palette,
            ),
            Some((palette.conflict_fill, 2))
        );
        assert_eq!(
            merge_result_background_run(
                &styles,
                1,
                0,
                styles.len(),
                MergeHighlightMode::Lines,
                palette,
            ),
            None
        );
    }

    #[test]
    fn result_editor_uses_display_tones_for_base_only_and_conflict_rows() {
        let source = include_str!("merge_tool.rs");
        let result_panel = source
            .split("fn merge_result_panel")
            .nth(1)
            .and_then(|tail| tail.split("fn result_header").next())
            .expect("result panel source");
        let row_painter = source
            .split("fn merge_editable_result_row")
            .nth(1)
            .and_then(|tail| tail.split("#[cfg(test)]").next())
            .expect("result row source");

        assert!(result_panel.contains("merge_result_display_rows(&app.document)"));
        assert!(row_painter.contains("MergeSideLineTone::BaseOnly => palette.base_only_fill"));
        assert!(row_painter.contains("palette.active_conflict_fill"));
    }

    #[test]
    fn base_only_marker_line_uses_base_only_row_fill() {
        let source = include_str!("merge_tool.rs");
        let marker_source = source
            .split("fn paint_base_only_gap_marker_rect")
            .nth(1)
            .and_then(|tail| tail.split("fn base_only_gap_marker_rect").next())
            .expect("marker painter source");

        assert!(marker_source.contains("palette.base_only_fill"));
        assert!(!marker_source.contains("palette.base_only_text"));
    }

    #[test]
    fn result_connector_uses_display_rows_without_vertical_offset() {
        let document = three_way_merge(
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\nbuild/\ncache/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\n.vscode\nvite.config.mts.*.mjs\nnil\nCLAUDE.md\n.codegraph/*\nAGENTS.md\n.codex/*\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\ndist-local/\ncache-local/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nCLAUDE.local.md\n.codegraph-local/*\nAGENTS.local.md\n.codex-local/*\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
            "# Merge Tool Complex Fixture\n\nsection: stable header\nalpha: unchanged\nbeta: unchanged\n\nsection: shared ignore patterns\ndist/\nnode_modules/\nrelease-dist/\ngraph-cache/\ngraph-cost.json\ncache-remote/\n\nsection: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\n**/graphify-out/cache/\n**/graphify-out/cost.json\n\nsection: stable footer\nomega: unchanged\nzeta: unchanged\n",
        );
        let panel = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(360.0, 640.0));
        let first = merge_block_result_rect(panel, &document, &document.conflicts()[0], 0.0)
            .expect("first conflict line");
        let second = merge_block_result_rect(panel, &document, &document.conflicts()[1], 0.0)
            .expect("second conflict line");

        let content_top = merge_scroll_content_top(panel);
        assert_eq!(first.top(), content_top + 9.0 * MERGE_CODE_ROW_HEIGHT);
        assert_eq!(second.top(), content_top + 23.0 * MERGE_CODE_ROW_HEIGHT);
    }

    #[test]
    fn side_display_rows_show_replacements_as_replace_rows() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );

        let rows = merge_side_display_rows(&document, MergeSide::Local);
        let changed = rows
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();

        assert_eq!(
            changed,
            vec![
                ("dist-local/", MergeSideLineTone::Replaced),
                ("cache-local/", MergeSideLineTone::Replaced),
            ]
        );
    }

    #[test]
    fn extra_rows_inside_side_replacement_blocks_stay_replacements() {
        let document = three_way_merge(
            "keep\nclaude.md\n.codegraph/*\n.mcp.json\nAGENTS.md\n.codex/*\nend\n",
            "keep\nCLAUDE.local.md\n.codegraph-local/*\nAGENTS.local.md\n.codex-local/*\nend\n",
            "keep\n**/graphify-out/cache/\n**/graphify-out/cost.json\nend\n",
        );

        let local_rows = merge_side_display_rows(&document, MergeSide::Local)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();
        let remote_rows = merge_side_display_rows(&document, MergeSide::Remote)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();

        assert_eq!(
            local_rows,
            vec![
                ("CLAUDE.local.md", MergeSideLineTone::Replaced),
                (".codegraph-local/*", MergeSideLineTone::Replaced),
                ("AGENTS.local.md", MergeSideLineTone::Replaced),
                (".codex-local/*", MergeSideLineTone::Replaced),
            ]
        );
        assert_eq!(
            remote_rows,
            vec![
                ("**/graphify-out/cache/", MergeSideLineTone::Replaced),
                ("**/graphify-out/cost.json", MergeSideLineTone::Replaced),
            ]
        );
    }

    #[test]
    fn opposing_base_deletions_render_as_replacements() {
        let document = three_way_merge(
            "keep\nalpha: unchanged\nbeta: unchanged\nend\n",
            "keep\nbeta: unchanged\nend\n",
            "keep\nalpha: unchanged\nend\n",
        );

        assert_eq!(document.conflicts().len(), 1);
        assert_eq!(
            document.conflicts()[0].base,
            vec!["alpha: unchanged", "beta: unchanged"]
        );

        let local_rows = merge_side_display_rows(&document, MergeSide::Local)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone, row.line_number))
            .collect::<Vec<_>>();
        let remote_rows = merge_side_display_rows(&document, MergeSide::Remote)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone, row.line_number))
            .collect::<Vec<_>>();

        assert_eq!(
            local_rows,
            vec![("beta: unchanged", MergeSideLineTone::Replaced, Some(2))]
        );
        assert_eq!(
            remote_rows,
            vec![("alpha: unchanged", MergeSideLineTone::Replaced, Some(2))]
        );

        let result_rows = merge_result_display_rows(&document)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();
        assert_eq!(
            result_rows,
            vec![
                ("alpha: unchanged", MergeSideLineTone::Replaced),
                ("beta: unchanged", MergeSideLineTone::Replaced),
            ]
        );
    }

    #[test]
    fn unresolved_result_rows_keep_side_text_for_word_highlighting() {
        let document = three_way_merge(
            "keep\nruntime_mode = base\nendpoint = /v1/base\nend\n",
            "keep\nruntime_mode = local\nendpoint = /v1/local\nend\n",
            "keep\nruntime_mode = remote\nendpoint = /v2/remote\nend\n",
        );

        let rows = merge_result_display_rows(&document)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.reference_text))
            .collect::<Vec<_>>();

        assert_eq!(
            rows,
            vec![
                ("runtime_mode = base", Some("runtime_mode = local")),
                ("endpoint = /v1/base", Some("endpoint = /v1/local")),
            ]
        );
    }

    #[test]
    fn one_sided_base_deletions_render_as_base_only_rows() {
        let document = three_way_merge(
            "keep\nshared before\n.claude/*\n.agents/*\nbase replaced\nend\n",
            "keep\nshared before\nlocal replacement\nend\n",
            "keep\nshared before\n.claude/*\n.agents/*\nremote replacement\nend\n",
        );

        assert_eq!(document.conflicts().len(), 1);
        let result_rows = merge_result_display_rows(&document)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();

        assert_eq!(
            result_rows,
            vec![
                (".claude/*", MergeSideLineTone::BaseOnly),
                (".agents/*", MergeSideLineTone::BaseOnly),
                ("base replaced", MergeSideLineTone::Replaced),
            ]
        );
    }

    #[test]
    fn auto_resolved_one_sided_deletions_remain_visible_as_base_only_rows() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );

        assert!(document.conflicts().is_empty());
        assert_eq!(document.result_text(), "keep\nend\n");

        let result_rows = merge_result_display_rows(&document)
            .iter()
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();

        assert_eq!(
            result_rows,
            vec![
                ("keep", MergeSideLineTone::Unchanged),
                (".claude/*", MergeSideLineTone::BaseOnly),
                (".agents/*", MergeSideLineTone::BaseOnly),
                ("end", MergeSideLineTone::Unchanged),
            ]
        );
    }

    #[test]
    fn complex_auto_deleted_base_rows_remain_in_result_display_before_replacement_block() {
        let document = three_way_merge(
            "section: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nclaude.md\n.codegraph/*\n.mcp.json\nAGENTS.md\n.codex/*\nend\n",
            "section: agent files\n.vscode/*\n.cursor/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nCLAUDE.local.md\n.codegraph-local/*\nAGENTS.local.md\n.codex-local/*\nend\n",
            "section: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\n**/graphify-out/cache/\n**/graphify-out/cost.json\nend\n",
        );

        assert!(!document.result_text().contains(".claude/*"));
        assert!(!document.result_text().contains(".agents/*"));

        let rows = merge_result_display_rows(&document)
            .iter()
            .map(|row| (row.text, row.tone))
            .collect::<Vec<_>>();
        let start = rows
            .iter()
            .position(|row| row.0 == ".vscode/*")
            .expect("agent section start");

        assert_eq!(
            &rows[start..start + 12],
            &[
                (".vscode/*", MergeSideLineTone::Unchanged),
                (".cursor/*", MergeSideLineTone::Unchanged),
                (".claude/*", MergeSideLineTone::BaseOnly),
                (".agents/*", MergeSideLineTone::BaseOnly),
                ("!.vscode/extensions.json", MergeSideLineTone::Unchanged),
                (".idea", MergeSideLineTone::Unchanged),
                ("vite.config.mts.*.mjs", MergeSideLineTone::Unchanged),
                ("nil", MergeSideLineTone::Unchanged),
                ("claude.md", MergeSideLineTone::Replaced),
                (".codegraph/*", MergeSideLineTone::Replaced),
                (".mcp.json", MergeSideLineTone::Replaced),
                ("AGENTS.md", MergeSideLineTone::Replaced),
            ]
        );
    }

    #[test]
    fn base_only_side_rows_mark_missing_side_with_gap_rows() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );

        let local_rows = merge_side_display_rows(&document, MergeSide::Local)
            .iter()
            .map(|row| {
                (
                    row.text,
                    row.tone,
                    row.line_number,
                    row.show_conflict_actions,
                    row.action_target,
                )
            })
            .collect::<Vec<_>>();
        let remote_rows = merge_side_display_rows(&document, MergeSide::Remote)
            .iter()
            .map(|row| (row.text, row.tone, row.line_number))
            .collect::<Vec<_>>();

        assert_eq!(
            local_rows,
            vec![
                ("keep", MergeSideLineTone::Unchanged, Some(1), false, None),
                ("end", MergeSideLineTone::Unchanged, Some(2), false, None),
            ]
        );
        assert_eq!(
            remote_rows,
            vec![
                ("keep", MergeSideLineTone::Unchanged, Some(1)),
                (".claude/*", MergeSideLineTone::Unchanged, Some(2)),
                (".agents/*", MergeSideLineTone::Unchanged, Some(3)),
                ("end", MergeSideLineTone::Unchanged, Some(4)),
            ]
        );
    }

    #[test]
    fn base_only_marker_is_overlay_not_a_side_display_row() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let local_rows = merge_side_display_rows(&document, MergeSide::Local);

        assert!(
            !local_rows
                .iter()
                .any(|row| row.tone == MergeSideLineTone::BaseOnly && row.text.is_empty())
        );
        assert_eq!(
            local_rows.iter().map(|row| row.text).collect::<Vec<_>>(),
            vec!["keep", "end"]
        );
    }

    #[test]
    fn base_only_markers_use_overlay_geometry_without_display_spacers() {
        let source = include_str!("merge_tool.rs");
        let implementation = source
            .split("#[cfg(test)]")
            .next()
            .expect("implementation section");

        assert!(!implementation.contains("base_only_gap_rows"));
        assert!(implementation.contains("paint_base_only_side_overlays"));
        assert!(implementation.contains("MERGE_BASE_ONLY_MARKER_HEIGHT"));
    }

    #[test]
    fn base_only_group_actions_keep_or_restore_deleted_base_rows() {
        let mut document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );

        assert_eq!(document.result_text(), "keep\nend\n");

        document.drop_base_only_group(1, MergeSide::Local);
        assert_eq!(document.result_text(), "keep\n.claude/*\n.agents/*\nend\n");

        let mut document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        document.take_base_only_group(1, MergeSide::Local);
        assert_eq!(document.result_text(), "keep\nend\n");
    }

    #[test]
    fn undoing_base_only_action_restores_all_merge_display_rows() {
        let document = three_way_merge(
            "keep\nremoved line\nend\n",
            "keep\nend\n",
            "keep\nremoved line\nend\n",
        );
        let mut app = MergeToolApp::new(test_merge_args(), document);
        let initial_result_rows = app.manual_result_lines.clone();
        let initial_local_rows = app.local_display_rows.clone();
        let initial_remote_rows = app.remote_display_rows.clone();
        let initial_epoch = app.display_epoch;

        app.apply_line_action(
            MergeLineActionTarget::BaseOnlyGroup(1),
            MergeSide::Local,
            MergeLineAction::Take,
        );
        assert_ne!(app.manual_result_lines, initial_result_rows);
        assert_ne!(app.display_epoch, initial_epoch);

        assert!(app.undo());
        assert_eq!(app.manual_result_lines, initial_result_rows);
        assert_eq!(app.local_display_rows, initial_local_rows);
        assert_eq!(app.remote_display_rows, initial_remote_rows);
        assert!(app.display_epoch > initial_epoch);
    }

    #[test]
    fn taking_base_only_group_hides_result_rows_and_missing_side_marker() {
        let mut document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );

        assert!(
            merge_result_display_lines(&document)
                .iter()
                .any(|line| *line == ".claude/*")
        );
        assert!(
            base_only_display_groups(&document)
                .iter()
                .any(|group| group.missing_side == MergeSide::Local)
        );

        document.take_base_only_group(1, MergeSide::Local);

        assert_eq!(document.result_text(), "keep\nend\n");
        assert!(
            !merge_result_display_lines(&document)
                .iter()
                .any(|line| *line == ".claude/*" || *line == ".agents/*")
        );
        assert!(
            !base_only_display_groups(&document)
                .iter()
                .any(|group| group.missing_side == MergeSide::Local)
        );
        assert!(
            merge_side_display_rows(&document, MergeSide::Remote)
                .iter()
                .any(|row| row.text == ".claude/*" && row.tone == MergeSideLineTone::Unchanged)
        );
    }

    #[test]
    fn base_only_connector_rects_join_missing_marker_to_result_rows() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let group = base_only_display_groups(&document)
            .into_iter()
            .next()
            .expect("base-only deletion group");
        let result_panel = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(360.0, 640.0));
        let side_panel = Rect::from_min_size(Pos2::new(20.0, 40.0), Vec2::new(260.0, 640.0));

        assert_eq!(group.line_index, 1);
        assert_eq!(group.line_count, 2);
        assert_eq!(group.missing_side, MergeSide::Local);

        let result_rect =
            merge_base_only_result_rect(result_panel, &document, group, 0.0).expect("result rect");
        let side_rect =
            merge_base_only_side_rect(side_panel, &document, group, 0.0).expect("side rect");
        let content_top = merge_scroll_content_top(result_panel);

        assert_eq!(result_rect.top(), content_top + MERGE_CODE_ROW_HEIGHT);
        assert_eq!(
            result_rect.bottom(),
            content_top + MERGE_CODE_ROW_HEIGHT * 3.0
        );
        assert_eq!(side_rect.height(), MERGE_BASE_ONLY_MARKER_HEIGHT);
        assert_eq!(
            side_rect.top(),
            merge_scroll_content_top(side_panel) + MERGE_CODE_ROW_HEIGHT
                - MERGE_BASE_ONLY_MARKER_HEIGHT * 0.5
        );
        assert_eq!(
            side_rect.left(),
            merge_scroll_clip_rect(side_panel).left() + 58.0
        );
        assert_eq!(
            side_rect.right(),
            merge_scroll_clip_rect(side_panel).right() - 8.0
        );
    }

    #[test]
    fn base_only_marker_bridge_connects_marker_edges_to_result_edges() {
        let result_rect = Rect::from_min_max(Pos2::new(100.0, 40.0), Pos2::new(260.0, 76.0));
        let marker_rect = Rect::from_min_max(Pos2::new(20.0, 50.0), Pos2::new(80.0, 53.0));

        assert_eq!(
            base_only_marker_bridge_points(result_rect, marker_rect, MergeSide::Local),
            vec![
                Pos2::new(100.0, 40.0),
                Pos2::new(90.0, 40.0),
                Pos2::new(80.0, 50.0),
                Pos2::new(80.0, 53.0),
                Pos2::new(90.0, 76.0),
                Pos2::new(100.0, 76.0),
            ]
        );
        assert_eq!(
            base_only_marker_bridge_points(result_rect, marker_rect, MergeSide::Remote),
            vec![
                Pos2::new(260.0, 40.0),
                Pos2::new(140.0, 40.0),
                Pos2::new(20.0, 50.0),
                Pos2::new(20.0, 53.0),
                Pos2::new(140.0, 76.0),
                Pos2::new(260.0, 76.0),
            ]
        );
    }

    #[test]
    fn remote_base_only_connector_rect_joins_missing_marker_to_result_rows() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
        );
        let group = base_only_display_groups(&document)
            .into_iter()
            .next()
            .expect("base-only deletion group");
        let side_panel = Rect::from_min_size(Pos2::new(500.0, 40.0), Vec2::new(260.0, 640.0));

        assert_eq!(group.missing_side, MergeSide::Remote);

        let side_rect =
            merge_base_only_side_rect(side_panel, &document, group, 0.0).expect("side rect");

        assert_eq!(side_rect.height(), MERGE_BASE_ONLY_MARKER_HEIGHT);
        assert_eq!(
            side_rect.left(),
            merge_scroll_clip_rect(side_panel).left() + 58.0
        );
    }

    #[test]
    fn side_scroll_offset_maps_from_result_visible_rows() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let result_scroll_y = MERGE_CODE_ROW_HEIGHT * 3.0;
        let compressed_side_scroll_y = MERGE_CODE_ROW_HEIGHT;

        assert_eq!(
            merge_side_scroll_y_for_result_scroll(&document, MergeSide::Local, result_scroll_y),
            compressed_side_scroll_y
        );
        assert_eq!(
            merge_side_scroll_y_for_result_scroll(&document, MergeSide::Remote, result_scroll_y),
            result_scroll_y
        );
        assert_eq!(
            merge_result_scroll_y_for_side_scroll(
                &document,
                MergeSide::Local,
                compressed_side_scroll_y
            ),
            compressed_side_scroll_y
        );
    }

    #[test]
    fn side_rows_after_base_only_gap_keep_result_row_alignment() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let end_index = document
            .lines
            .iter()
            .position(|line| line.result == "end")
            .expect("end line");

        assert_eq!(
            merge_result_display_row_for_line(&document, end_index),
            Some(3)
        );
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Local, end_index),
            Some(1)
        );
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Remote, end_index),
            Some(3)
        );
    }

    #[test]
    fn base_only_gap_lines_keep_distinct_scroll_anchors() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );

        assert_eq!(merge_result_display_row_for_line(&document, 1), Some(1));
        assert_eq!(merge_result_display_row_for_line(&document, 2), Some(2));
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Local, 1),
            Some(1)
        );
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Local, 2),
            Some(1)
        );
        assert_eq!(
            merge_side_scroll_y_for_result_scroll(
                &document,
                MergeSide::Local,
                MERGE_CODE_ROW_HEIGHT * 2.0
            ),
            MERGE_CODE_ROW_HEIGHT
        );
    }

    #[test]
    fn base_only_gap_marker_does_not_consume_side_row_height() {
        let document = three_way_merge(
            "keep\n.claude/*\n.agents/*\nend\n",
            "keep\nend\n",
            "keep\n.claude/*\n.agents/*\nend\n",
        );
        let local_rows = merge_side_display_rows(&document, MergeSide::Local);

        assert!(
            !local_rows
                .iter()
                .any(|row| row.tone == MergeSideLineTone::BaseOnly && row.text.is_empty())
        );
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Local, 3),
            Some(1)
        );
    }

    #[test]
    fn first_unchanged_line_after_base_only_gap_stays_scroll_aligned() {
        let document = three_way_merge(
            "section: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nclaude.md\n.codegraph/*\n.mcp.json\nAGENTS.md\n.codex/*\nend\n",
            "section: agent files\n.vscode/*\n.cursor/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\nCLAUDE.local.md\n.codegraph-local/*\nAGENTS.local.md\n.codex-local/*\nend\n",
            "section: agent files\n.vscode/*\n.cursor/*\n.claude/*\n.agents/*\n!.vscode/extensions.json\n.idea\nvite.config.mts.*.mjs\nnil\n**/graphify-out/cache/\n**/graphify-out/cost.json\nend\n",
        );
        let line_index = document
            .lines
            .iter()
            .position(|line| line.result == "!.vscode/extensions.json")
            .expect("first shared line after base-only gap");
        let result_row = merge_result_display_row_for_line(&document, line_index);

        assert_eq!(result_row, Some(5));
        assert_eq!(
            merge_side_display_row_for_line(&document, MergeSide::Local, line_index),
            Some(3)
        );
        assert_eq!(
            result_row,
            merge_side_display_row_for_line(&document, MergeSide::Remote, line_index)
        );
        let scroll_y = result_row.expect("result row") as f32 * MERGE_CODE_ROW_HEIGHT;
        assert_eq!(
            merge_side_scroll_y_for_result_scroll(&document, MergeSide::Local, scroll_y),
            MERGE_CODE_ROW_HEIGHT * 3.0
        );
        let fractional_scroll_y = scroll_y - MERGE_CODE_ROW_HEIGHT * 0.5;
        assert_eq!(
            merge_side_scroll_y_for_result_scroll(&document, MergeSide::Local, fractional_scroll_y),
            MERGE_CODE_ROW_HEIGHT * 3.0
        );
    }

    #[test]
    fn side_connector_rect_uses_side_display_rows() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );
        let panel = Rect::from_min_size(Pos2::new(20.0, 40.0), Vec2::new(260.0, 420.0));
        let rect = merge_block_side_rect(
            panel,
            &document,
            &document.conflicts()[0],
            MergeSide::Local,
            0.0,
        )
        .expect("side connector rect");
        let top = merge_scroll_content_top(panel) + MERGE_CODE_ROW_HEIGHT;

        assert_eq!(rect.top(), top);
        assert_eq!(rect.bottom(), top + MERGE_CODE_ROW_HEIGHT * 2.0);
    }

    #[test]
    fn bridge_connects_final_rect_edges_without_late_side_offset() {
        let source = include_str!("merge_tool.rs");
        let implementation = source
            .split("fn paint_side_block_bridge")
            .nth(1)
            .and_then(|tail| tail.split("fn merge_connector_color").next())
            .expect("bridge implementation");

        assert!(implementation.contains("Pos2::new(side_x, side_rect.top())"));
        assert!(implementation.contains("Pos2::new(side_x, side_rect.bottom())"));
        assert!(!implementation.contains("fn merge_connector_side_y("));
        assert!(!implementation.contains("fn merge_connector_side_bottom_y("));
    }

    #[test]
    fn base_only_marker_uses_boundary_anchor_not_block_bridge() {
        let source = include_str!("merge_tool.rs");
        let base_only_section = source
            .split("fn paint_merge_block_connectors")
            .nth(1)
            .and_then(|tail| tail.split("fn merge_connector_debug_mode").next())
            .and_then(|tail| {
                tail.split("for group in base_only_display_groups(document)")
                    .nth(1)
            })
            .expect("base-only connector loop");

        assert!(base_only_section.contains("paint_base_only_marker_bridge"));
        assert!(!base_only_section.contains("paint_side_block_bridge"));
    }

    #[test]
    fn connector_debug_mode_uses_build_time_configuration() {
        assert_eq!(
            merge_build_config_bool("connector_guides"),
            MERGE_BUILD_CONFIG.contains("connector_guides = true")
        );
    }

    #[test]
    fn connector_debug_mode_is_read_from_build_config() {
        let source = include_str!("merge_tool.rs");
        let shortcuts = source
            .split("fn handle_keyboard_shortcuts")
            .nth(1)
            .and_then(|tail| tail.split("fn handle_close_request").next())
            .expect("merge shortcut implementation");
        assert!(source.contains("connector_debug: MergeConnectorDebug"));
        assert!(source.contains("include_str!(\"../config/merge-tool.toml\")"));
        assert!(source.contains("merge_build_config_bool(\"connector_guides\")"));
        assert!(!shortcuts.contains("Key::D"));
    }

    #[test]
    fn conflict_action_buttons_are_limited_to_block_start() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );

        let flags = merge_side_display_rows(&document, MergeSide::Remote)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| row.show_conflict_actions)
            .collect::<Vec<_>>();

        assert_eq!(flags, vec![true, false]);
    }

    #[test]
    fn deleted_side_display_rows_do_not_take_side_line_numbers() {
        let document = three_way_merge(
            "keep\nbuild/\ncache/\nend\n",
            "keep\ndist-local/\ncache-local/\nend\n",
            "keep\nrelease-dist/\ngraph-cache/\nend\n",
        );

        let line_numbers = merge_side_display_rows(&document, MergeSide::Local)
            .iter()
            .filter(|row| row.conflict_index == Some(0))
            .map(|row| (row.tone, row.line_number))
            .collect::<Vec<_>>();

        assert_eq!(
            line_numbers,
            vec![
                (MergeSideLineTone::Replaced, Some(2)),
                (MergeSideLineTone::Replaced, Some(3)),
            ]
        );
    }

    #[test]
    fn merge_tool_scroll_and_spacing_are_shared_and_dense() {
        let source = include_str!("merge_tool.rs");
        let implementation = source
            .split("#[cfg(test)]")
            .next()
            .expect("implementation section");

        assert!(implementation.contains("shared_scroll_y"));
        assert!(implementation.contains(".vertical_scroll_offset(scroll_y)"));
        assert!(implementation.contains("item_spacing.y = 0.0"));
        assert!(implementation.contains("MERGE_CODE_ROW_HEIGHT"));
        assert!(!implementation.contains("let row_h = 22.0"));
        assert!(!implementation.contains("ui.add_space(30.0)"));
    }

    #[test]
    fn remote_conflict_actions_put_take_before_drop() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(240.0, 18.0));

        let local = conflict_action_rects(rect, MergeSide::Local);
        assert!(local.drop.left() < local.take.left());
        assert_eq!(local.drop.top(), rect.top());
        assert_eq!(local.drop.bottom(), rect.bottom());

        let remote = conflict_action_rects(rect, MergeSide::Remote);
        assert!(remote.take.left() < remote.drop.left());
        assert_eq!(remote.take.top(), rect.top());
        assert_eq!(remote.take.bottom(), rect.bottom());
    }

    #[test]
    fn result_panel_paints_conflict_connectors() {
        let source = include_str!("merge_tool.rs");
        let implementation = source
            .split("mod tests")
            .next()
            .expect("implementation section");

        assert!(implementation.contains("paint_merge_block_connectors"));
        assert!(implementation.contains("merge_block_result_rect"));
        assert!(!implementation.contains("fn paint_result_connector("));
        assert!(!implementation.contains("fn paint_side_connector("));
        assert!(!implementation.contains("palette.connector.gamma_multiply(0.10)"));
        assert!(implementation.contains("Shape::convex_polygon"));
        assert!(implementation.contains("merge_block_connector_tone"));
        assert!(implementation.contains("paint_result_block_outline(ui, result_rect, tone"));
    }

    #[test]
    fn merge_connectors_use_real_row_geometry_for_current_frame() {
        let source = include_str!("merge_tool.rs");
        let implementation = source
            .split("fn merge_editor_columns")
            .nth(1)
            .and_then(|tail| tail.split("fn merge_side_panel").next())
            .expect("merge editor columns implementation");

        assert!(implementation.contains("let requested_scroll_y = app.shared_scroll_y;"));
        assert!(implementation.contains("let frame_scroll_y = result_output.scroll_y;"));
        assert!(implementation.contains("&local_output.geometry"));
        assert!(implementation.contains("&result_output.geometry"));
        assert!(implementation.contains("&remote_output.geometry"));
        assert!(implementation.contains("app.shared_scroll_y = next_shared_scroll_y;"));
    }
}
