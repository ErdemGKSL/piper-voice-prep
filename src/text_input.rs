use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct TextInput {
    text: String,
    cursor: usize, // Grapheme index, so accented and combined characters stay intact.
    anchor: Option<usize>,
}

impl TextInput {
    pub fn set(&mut self, text: String) {
        self.cursor = text.graphemes(true).count();
        self.text = text;
        self.anchor = None;
    }

    pub fn value(&self) -> &str {
        &self.text
    }

    fn count(&self) -> usize {
        self.text.graphemes(true).count()
    }

    fn offset(&self, index: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.text.len())
    }

    fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|anchor| *anchor != self.cursor)
            .map(|anchor| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection() {
            let from = self.offset(start);
            let to = self.offset(end);
            self.text.replace_range(from..to, "");
            self.cursor = start;
            self.anchor = None;
            true
        } else {
            false
        }
    }

    fn move_to(&mut self, cursor: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = cursor.min(self.count());
    }

    fn word_left(&self) -> usize {
        let parts: Vec<&str> = self.text.graphemes(true).collect();
        let mut i = self.cursor;
        while i > 0 && parts[i - 1].chars().all(char::is_whitespace) {
            i -= 1;
        }
        while i > 0 && !parts[i - 1].chars().all(char::is_whitespace) {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let parts: Vec<&str> = self.text.graphemes(true).collect();
        let mut i = self.cursor;
        while i < parts.len() && !parts[i].chars().all(char::is_whitespace) {
            i += 1;
        }
        while i < parts.len() && parts[i].chars().all(char::is_whitespace) {
            i += 1;
        }
        i
    }

    pub fn insert(&mut self, value: &str) {
        self.delete_selection();
        let clean = value.replace(['\r', '\n', '\t', '|'], " ");
        let at = self.offset(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor = self.text[..at + clean.len()].graphemes(true).count();
    }

    pub fn handle(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Left => self.move_to(
                if ctrl {
                    self.word_left()
                } else {
                    self.cursor.saturating_sub(1)
                },
                shift,
            ),
            KeyCode::Right => self.move_to(
                if ctrl {
                    self.word_right()
                } else {
                    self.cursor + 1
                },
                shift,
            ),
            KeyCode::Home => self.move_to(0, shift),
            KeyCode::End => self.move_to(self.count(), shift),
            KeyCode::Char('a') if ctrl => {
                self.anchor = Some(0);
                self.cursor = self.count();
            }
            KeyCode::Char('w') if ctrl => {
                if !self.delete_selection() {
                    let start = self.word_left();
                    let from = self.offset(start);
                    let to = self.offset(self.cursor);
                    self.text.replace_range(from..to, "");
                    self.cursor = start;
                }
            }
            KeyCode::Backspace => {
                if !self.delete_selection() && self.cursor > 0 {
                    let start = if ctrl {
                        self.word_left()
                    } else {
                        self.cursor - 1
                    };
                    let from = self.offset(start);
                    let to = self.offset(self.cursor);
                    self.text.replace_range(from..to, "");
                    self.cursor = start;
                }
            }
            KeyCode::Delete => {
                if !self.delete_selection() && self.cursor < self.count() {
                    let from = self.offset(self.cursor);
                    let to = self.offset(if ctrl {
                        self.word_right()
                    } else {
                        self.cursor + 1
                    });
                    self.text.replace_range(from..to, "");
                }
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }

    pub fn view(&self, width: usize) -> (Line<'static>, u16) {
        if width == 0 {
            return (Line::default(), 0);
        }
        let parts: Vec<&str> = self.text.graphemes(true).collect();
        let mut start = 0;
        let mut cursor_x = UnicodeWidthStr::width(parts[..self.cursor].concat().as_str());
        while cursor_x >= width && start < self.cursor {
            cursor_x = cursor_x.saturating_sub(UnicodeWidthStr::width(parts[start]));
            start += 1;
        }
        let selected = self.selection();
        let mut spans = Vec::new();
        let mut used = 0;
        for (i, part) in parts.iter().enumerate().skip(start) {
            let size = UnicodeWidthStr::width(*part);
            if used + size > width {
                break;
            }
            let style = if selected.is_some_and(|(a, b)| i >= a && i < b) {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled((*part).to_owned(), style));
            used += size;
        }
        (Line::from(spans), cursor_x.min(width - 1) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn edits_at_cursor_and_preserves_graphemes() {
        let mut input = TextInput::default();
        input.set("aö👩‍💻z".into());
        input.handle(key(KeyCode::Left, KeyModifiers::NONE));
        input.handle(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(input.value(), "aöz");
        input.insert("X");
        assert_eq!(input.value(), "aöXz");
        input.handle(key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(input.value(), "aöX");
    }

    #[test]
    fn selection_replaces_text() {
        let mut input = TextInput::default();
        input.set("hello world".into());
        input.handle(key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        input.insert("new");
        assert_eq!(input.value(), "new");
        input.handle(key(KeyCode::Home, KeyModifiers::NONE));
        input.handle(key(KeyCode::Right, KeyModifiers::SHIFT));
        input.insert("N");
        assert_eq!(input.value(), "New");
    }
}
