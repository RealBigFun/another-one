//! Shared byte-index-safe text editing primitives for app editable fields.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorDirection {
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextEditState {
    pub cursor: usize,
    pub selection_anchor: Option<usize>,
}

impl TextEditState {
    pub(crate) fn new(cursor: usize, selection_anchor: Option<usize>) -> Self {
        Self {
            cursor,
            selection_anchor,
        }
    }

    pub(crate) fn clamp_to_text(&mut self, text: &str) {
        self.cursor = clamp_boundary(text, self.cursor);
        self.selection_anchor = self
            .selection_anchor
            .map(|anchor| clamp_boundary(text, anchor))
            .filter(|anchor| *anchor != self.cursor);
    }

    pub(crate) fn selected_range(&self) -> Option<Range<usize>> {
        selected_range(self.cursor, self.selection_anchor)
    }

    pub(crate) fn select_all(&mut self, text: &str) {
        self.cursor = text.len();
        self.selection_anchor = Some(0).filter(|_| !text.is_empty());
    }

    pub(crate) fn replace_range(
        &mut self,
        text: &mut String,
        range: Range<usize>,
        replacement: &str,
    ) {
        let start = clamp_boundary(text, range.start);
        let end = clamp_boundary(text, range.end).max(start);
        text.replace_range(start..end, replacement);
        self.cursor = start + replacement.len();
        self.selection_anchor = None;
    }

    pub(crate) fn insert_text(&mut self, text: &mut String, inserted: &str) {
        self.clamp_to_text(text);
        let range = self.selected_range().unwrap_or(self.cursor..self.cursor);
        self.replace_range(text, range, inserted);
    }

    pub(crate) fn delete_backward(&mut self, text: &mut String) {
        self.clamp_to_text(text);
        if let Some(range) = self.selected_range() {
            self.replace_range(text, range, "");
        } else if self.cursor > 0 {
            let start = previous_boundary(text, self.cursor);
            self.replace_range(text, start..self.cursor, "");
        }
    }

    pub(crate) fn delete_word_backward(&mut self, text: &mut String) {
        self.clamp_to_text(text);
        if let Some(range) = self.selected_range() {
            self.replace_range(text, range, "");
        } else if self.cursor > 0 {
            let start = previous_word_boundary(text, self.cursor);
            self.replace_range(text, start..self.cursor, "");
        }
    }

    pub(crate) fn delete_to_start(&mut self, text: &mut String) {
        self.clamp_to_text(text);
        if let Some(range) = self.selected_range() {
            self.replace_range(text, range, "");
        } else if self.cursor > 0 {
            self.replace_range(text, 0..self.cursor, "");
        }
    }

    pub(crate) fn delete_forward(&mut self, text: &mut String) {
        self.clamp_to_text(text);
        if let Some(range) = self.selected_range() {
            self.replace_range(text, range, "");
        } else if self.cursor < text.len() {
            let end = next_boundary(text, self.cursor);
            self.replace_range(text, self.cursor..end, "");
        }
    }

    pub(crate) fn move_horizontal(
        &mut self,
        text: &str,
        direction: CursorDirection,
        extend_selection: bool,
    ) {
        self.clamp_to_text(text);
        let next_cursor = match direction {
            CursorDirection::Left => match self.selected_range() {
                Some(range) if !extend_selection => range.start,
                _ => previous_boundary(text, self.cursor),
            },
            CursorDirection::Right => match self.selected_range() {
                Some(range) if !extend_selection => range.end,
                _ => next_boundary(text, self.cursor),
            },
        };
        self.apply_cursor(next_cursor, extend_selection);
    }

    pub(crate) fn move_to_edge(&mut self, text: &str, to_end: bool, extend_selection: bool) {
        self.clamp_to_text(text);
        self.apply_cursor(if to_end { text.len() } else { 0 }, extend_selection);
    }

    pub(crate) fn move_vertical(&mut self, text: &str, up: bool, extend_selection: bool) {
        self.clamp_to_text(text);
        let current_line_start = line_start(text, self.cursor);
        let current_line_end = line_end(text, self.cursor);
        let current_column = char_count(&text[current_line_start..self.cursor]);
        let next = if up {
            if current_line_start == 0 {
                0
            } else {
                let previous_end = current_line_start.saturating_sub(1);
                let previous_start = line_start(text, previous_end);
                index_for_char_column(&text[previous_start..previous_end], current_column)
                    + previous_start
            }
        } else if current_line_end >= text.len() {
            text.len()
        } else {
            let next_start = current_line_end + 1;
            let next_end = line_end(text, next_start);
            index_for_char_column(&text[next_start..next_end], current_column) + next_start
        };
        self.apply_cursor(next, extend_selection);
    }

    pub(crate) fn move_to_line_edge(&mut self, text: &str, to_end: bool, extend_selection: bool) {
        self.clamp_to_text(text);
        let next = if to_end {
            line_end(text, self.cursor)
        } else {
            line_start(text, self.cursor)
        };
        self.apply_cursor(next, extend_selection);
    }

    fn apply_cursor(&mut self, next_cursor: usize, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        if !extend_selection {
            self.selection_anchor = None;
        }
        self.cursor = next_cursor;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
    }
}

pub(crate) fn selected_range(
    cursor: usize,
    selection_anchor: Option<usize>,
) -> Option<Range<usize>> {
    let anchor = selection_anchor?;
    if anchor == cursor {
        None
    } else if anchor < cursor {
        Some(anchor..cursor)
    } else {
        Some(cursor..anchor)
    }
}

pub(crate) fn previous_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(text, cursor);
    text.char_indices()
        .rev()
        .find_map(|(i, _)| (i < cursor).then_some(i))
        .unwrap_or(0)
}

pub(crate) fn next_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(text, cursor);
    text.char_indices()
        .find_map(|(i, _)| (i > cursor).then_some(i))
        .unwrap_or(text.len())
}

pub(crate) fn previous_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_boundary(text, cursor);
    while idx > 0 {
        let start = previous_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if !ch.is_whitespace() {
            break;
        }
        idx = start;
    }
    while idx > 0 {
        let start = previous_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '_' | '-') {
            idx = start;
        } else {
            break;
        }
    }
    idx
}

pub(crate) fn clamp_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        text.len()
    } else if text.is_char_boundary(index) {
        index
    } else {
        previous_boundary(text, index)
    }
}

pub(crate) fn char_count(text: &str) -> usize {
    text.chars().count()
}

pub(crate) fn index_for_char_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .map(|(idx, _)| idx)
        .nth(column)
        .unwrap_or(text.len())
}

pub(crate) fn line_start(text: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(text, cursor);
    text[..cursor].rfind('\n').map_or(0, |idx| idx + 1)
}

pub(crate) fn line_end(text: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(text, cursor);
    text[cursor..]
        .find('\n')
        .map_or(text.len(), |offset| cursor + offset)
}

pub(crate) fn line_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_empty() {
        return vec![0..0];
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            ranges.push(start..idx);
            start = idx + ch.len_utf8();
        }
    }
    ranges.push(start..text.len());
    ranges
}

pub(crate) fn visible_range(
    text: &str,
    cursor: usize,
    selection: Option<&Range<usize>>,
    max_chars: usize,
) -> Range<usize> {
    let boundaries = text
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let total_chars = boundaries.len().saturating_sub(1);
    if total_chars <= max_chars {
        return 0..text.len();
    }
    let cursor = clamp_boundary(text, cursor);
    let cursor_char = text[..cursor].chars().count();
    let mut start_char = cursor_char.saturating_sub(max_chars / 2);
    let mut end_char = (start_char + max_chars).min(total_chars);
    start_char = end_char.saturating_sub(max_chars);
    if cursor_char >= total_chars.saturating_sub(max_chars / 3) {
        end_char = total_chars;
        start_char = total_chars.saturating_sub(max_chars);
    }
    if let Some(selection) = selection {
        let selection_start_char = text[..selection.start.min(text.len())].chars().count();
        let selection_end_char = text[..selection.end.min(text.len())].chars().count();
        if selection_start_char < start_char {
            start_char = selection_start_char;
            end_char = (start_char + max_chars).min(total_chars);
        }
        if selection_end_char > end_char {
            end_char = selection_end_char.min(total_chars);
            start_char = end_char.saturating_sub(max_chars);
        }
    }
    boundaries[start_char]..boundaries[end_char]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_replaces_selection_and_clears_anchor() {
        let mut text = "--".to_string();
        let mut edit = TextEditState::new(2, Some(1));
        edit.insert_text(&mut text, "agent");
        assert_eq!(text, "-agent");
        assert_eq!(edit.cursor, "-agent".len());
        assert_eq!(edit.selection_anchor, None);
    }

    #[test]
    fn delete_backward_respects_utf8_boundaries() {
        let mut text = "aéb".to_string();
        let mut edit = TextEditState::new("aé".len(), None);
        edit.delete_backward(&mut text);
        assert_eq!(text, "ab");
        assert_eq!(edit.cursor, 1);
    }

    #[test]
    fn paste_sanitization_can_be_applied_by_caller() {
        let mut text = "run".to_string();
        let mut edit = TextEditState::new(text.len(), None);
        let pasted = " one\ntwo".replace(['\n', '\r', '\t'], " ");
        edit.insert_text(&mut text, &pasted);
        assert_eq!(text, "run one two");
    }

    #[test]
    fn vertical_movement_preserves_character_column() {
        let text = "αβγ\nde";
        let mut edit = TextEditState::new("αβ".len(), None);
        edit.move_vertical(text, false, false);
        assert_eq!(edit.cursor, text.len());
    }
}
