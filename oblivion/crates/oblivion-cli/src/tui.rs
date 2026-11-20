//! `oblivion --tui`: a full-screen terminal interface.
//!
//! Layout: a top bar (folder, model, approval mode, status), the conversation on
//! the left, a side panel (tasks, helpers, models) on the right, the input box at
//! the bottom, and a pop-up when an action needs approval.
//!
//! Keys: Enter send · Shift/Alt+Enter new line · Esc stop (or clear input) ·
//! PgUp/PgDn scroll · Up/Down history · Tab complete a /command ·
//! y / a / n answer an approval · Ctrl+C stop, then quit.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use futures_util::StreamExt;
use oblivion_core::Decision;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::{Controls, StateView, TodoView, UiEvent, Worker};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const SIDEBAR_WIDTH: u16 = 36;
const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const COMMANDS: &[&str] = &[
    "help", "undo", "models", "todo", "compact", "model", "approval", "clear", "session", "skills",
    "mcp", "exit",
];
/// Lines of an edit's diff shown inline in the conversation.
const DIFF_LINES: usize = 14;

#[derive(Debug, Clone, PartialEq)]
enum ToolStatus {
    Running,
    Ok,
    Err,
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
enum Item {
    User(String),
    Bot(String),
    Tool {
        name: String,
        summary: String,
        detail: String,
        status: ToolStatus,
        preview: String,
    },
    Note(String, Color),
    Info(String),
    End(String, Color),
}

#[derive(Debug, Clone, Default)]
struct Lane {
    label: String,
    last: String,
    done: bool,
}

#[derive(Debug, Clone)]
struct Approval {
    id: u64,
    tool: String,
    summary: String,
}

/// Everything the screen shows; updated by [`Model::apply`] and key presses.
#[derive(Default)]
pub struct Model {
    items: Vec<Item>,
    bot_open: bool,
    todos: Vec<TodoView>,
    lanes: BTreeMap<usize, Lane>,
    state: Option<StateView>,
    model_label: String,
    busy: bool,
    busy_since: Option<Instant>,
    approvals: VecDeque<Approval>,
    input: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    hist_pos: Option<usize>,
    /// Lines scrolled up from the bottom (0 = follow new output).
    scroll: usize,
    frame: usize,
    quit: bool,
}

impl Model {
    fn close_bot(&mut self) {
        self.bot_open = false;
    }

    fn note(&mut self, text: impl Into<String>, color: Color) {
        self.close_bot();
        self.items.push(Item::Note(text.into(), color));
    }

    fn running_tool(&mut self, name: &str) -> Option<&mut Item> {
        let pos = self.items.iter().rposition(
            |i| matches!(i, Item::Tool { status: ToolStatus::Running, name: n, .. } if n == name),
        )?;
        self.items.get_mut(pos)
    }

    pub fn apply(&mut self, e: UiEvent) {
        match e {
            UiEvent::User { text } => {
                self.close_bot();
                self.items.push(Item::User(text));
                self.scroll = 0;
            }
            UiEvent::Text { text } => {
                if self.bot_open {
                    if let Some(Item::Bot(t)) = self.items.last_mut() {
                        t.push_str(&text);
                        return;
                    }
                }
                self.items.push(Item::Bot(text));
                self.bot_open = true;
            }
            UiEvent::TextEnd => self.close_bot(),
            UiEvent::ToolStart {
                name,
                summary,
                detail,
            } => {
                self.close_bot();
                self.items.push(Item::Tool {
                    name,
                    summary,
                    detail,
                    status: ToolStatus::Running,
                    preview: String::new(),
                });
            }
            UiEvent::ToolEnd {
                name,
                preview,
                is_error,
                ..
            } => {
                if let Some(Item::Tool {
                    status, preview: p, ..
                }) = self.running_tool(&name)
                {
                    *status = if is_error {
                        ToolStatus::Err
                    } else {
                        ToolStatus::Ok
                    };
                    if name != "todo" {
                        *p = preview;
                    }
                }
            }
            UiEvent::Declined { name } => {
                if let Some(Item::Tool { status, .. }) = self.running_tool(&name) {
                    *status = ToolStatus::Denied;
                }
            }
            UiEvent::Blocked { name, rule } => {
                self.note(format!("⛔ {name} blocked by rule {rule}"), Color::Red)
            }
            UiEvent::Fallback { from, reason } => self.note(
                format!("↷ {from} failed ({reason}); trying the next model"),
                MUTED,
            ),
            UiEvent::Model { label } => self.model_label = label,
            UiEvent::Notice { text } => self.note(format!("! {text}"), Color::Yellow),
            UiEvent::Waiting { secs } => self.note(
                format!("⏳ all models are at their per-minute limit; waiting {secs}s"),
                MUTED,
            ),
            UiEvent::Sub {
                index,
                label,
                event,
            } => {
                let lane = self.lanes.entry(index).or_insert(Lane {
                    label,
                    last: "starting…".into(),
                    done: false,
                });
                match *event {
                    UiEvent::ToolStart { name, summary, .. } => {
                        lane.last = format!("{name} {summary}");
                        lane.done = false;
                    }
                    UiEvent::ToolEnd {
                        is_error: true,
                        preview,
                        ..
                    } => lane.last = format!("✗ {}", preview.lines().next().unwrap_or("")),
                    UiEvent::Waiting { secs } => lane.last = format!("waiting {secs}s"),
                    UiEvent::Model { label } => lane.last = format!("using {label}"),
                    UiEvent::TextEnd => {
                        lane.last = "finished".into();
                        lane.done = true;
                    }
                    _ => {}
                }
            }
            UiEvent::TurnEnd {
                steps,
                tool_calls,
                prompt_tokens,
                completion_tokens,
                outcome,
                error,
            } => {
                self.close_bot();
                for item in &mut self.items {
                    if let Item::Tool { status, .. } = item {
                        if *status == ToolStatus::Running {
                            *status = ToolStatus::Err;
                        }
                    }
                }
                for lane in self.lanes.values_mut() {
                    lane.done = true;
                }
                let (text, color) = match outcome.as_str() {
                    "cancelled" => (
                        "Stopped. File changes so far are kept; /undo reverts them.".to_string(),
                        Color::Yellow,
                    ),
                    "error" => (format!("✗ {}", error.unwrap_or_default()), Color::Red),
                    _ => {
                        let tokens = if prompt_tokens + completion_tokens > 0 {
                            format!(" · {prompt_tokens} in / {completion_tokens} out tokens")
                        } else {
                            String::new()
                        };
                        (
                            format!("{steps} step(s) · {tool_calls} tool call(s){tokens}"),
                            MUTED,
                        )
                    }
                };
                self.items.push(Item::End(text, color));
            }
            UiEvent::Approval {
                id, tool, summary, ..
            } => self.approvals.push_back(Approval { id, tool, summary }),
            UiEvent::ApprovalDone { id, .. } => self.approvals.retain(|a| a.id != id),
            UiEvent::Info { text } => {
                self.close_bot();
                self.items.push(Item::Info(text));
            }
            UiEvent::Todos { items } => self.todos = items,
            UiEvent::Busy { busy } => {
                self.busy = busy;
                self.busy_since = busy.then(Instant::now);
                if busy {
                    self.lanes.clear();
                }
            }
            UiEvent::State(s) => {
                self.todos = s.todos.clone();
                self.state = Some(s);
            }
            UiEvent::SessionResumed { .. } => {
                self.items.clear();
                self.lanes.clear();
            }
            UiEvent::Quit => self.quit = true,
        }
    }

    fn input_text(&self) -> String {
        self.input.iter().collect()
    }

    fn set_input(&mut self, s: &str) {
        self.input = s.chars().collect();
        self.cursor = self.input.len();
    }

    /// Slash commands and skills matching what is typed after "/".
    fn suggestions(&self) -> Vec<String> {
        let text = self.input_text();
        let Some(q) = text.strip_prefix('/') else {
            return vec![];
        };
        if q.contains(' ') {
            return vec![];
        }
        let skills = self
            .state
            .as_ref()
            .map(|s| s.skills.clone())
            .unwrap_or_default();
        let mut out: Vec<String> = COMMANDS
            .iter()
            .map(|c| c.to_string())
            .chain(skills)
            .filter(|c| c.starts_with(q) && c != q)
            .collect();
        out.dedup();
        out.truncate(8);
        out
    }

    /// Handle a key press; returns text to send to the agent, if any.
    pub fn key(&mut self, k: KeyEvent, controls: Option<&Controls>) -> Option<String> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(a) = self.approvals.front().cloned() {
            let decision = match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => Some(Decision::Yes),
                KeyCode::Char('a') | KeyCode::Char('A') => Some(Decision::Always),
                KeyCode::Char('n') | KeyCode::Char('N') => Some(Decision::No),
                KeyCode::Esc => Some(Decision::No),
                _ => None,
            };
            if let Some(d) = decision {
                if let Some(c) = controls {
                    c.approvals.answer(a.id, d);
                }
                self.approvals.pop_front();
                return None;
            }
            if !(ctrl && k.code == KeyCode::Char('c')) {
                return None;
            }
        }
        match k.code {
            KeyCode::Char('c') if ctrl => {
                if self.busy {
                    if let Some(c) = controls {
                        c.cancel();
                    }
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() => self.quit = true,
            KeyCode::Char('u') if ctrl => {
                self.input.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Esc => {
                if self.busy {
                    if let Some(c) = controls {
                        c.cancel();
                    }
                } else {
                    self.set_input("");
                }
            }
            KeyCode::Enter | KeyCode::Char('\n')
                if k.modifiers.contains(KeyModifiers::SHIFT)
                    || k.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.input.insert(self.cursor, '\n');
                self.cursor += 1;
            }
            KeyCode::Enter | KeyCode::Char('\r') | KeyCode::Char('\n') => {
                let text = self.input_text();
                if text.trim().is_empty() {
                    return None;
                }
                self.history.push(text.clone());
                self.hist_pos = None;
                self.set_input("");
                self.scroll = 0;
                return Some(text);
            }
            KeyCode::Tab => {
                if let Some(first) = self.suggestions().first() {
                    self.set_input(&format!("/{first} "));
                }
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.input.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.input.len() => {
                self.input.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Up if !self.history.is_empty() && !self.input.contains(&'\n') => {
                let pos = match self.hist_pos {
                    None => self.history.len() - 1,
                    Some(p) => p.saturating_sub(1),
                };
                self.hist_pos = Some(pos);
                let h = self.history[pos].clone();
                self.set_input(&h);
            }
            KeyCode::Down if self.hist_pos.is_some() => {
                let pos = self.hist_pos.unwrap() + 1;
                if pos >= self.history.len() {
                    self.hist_pos = None;
                    self.set_input("");
                } else {
                    self.hist_pos = Some(pos);
                    let h = self.history[pos].clone();
                    self.set_input(&h);
                }
            }
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Char(c) => {
                self.input.insert(self.cursor, c);
                self.cursor += 1;
            }
            _ => {}
        }
        None
    }

    pub fn paste(&mut self, text: &str) {
        let clean = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut chars = clean.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip ANSI escape sequence \x1b[...]
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            if !c.is_control() || c == '\n' || c == '\t' {
                self.input.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }
}

// ---------- drawing ----------

/// `**bold**` and `code` inside a line of the model's answer.
fn inline_spans(line: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_code = rest.find('`');
        let (pos, marker) = match (next_bold, next_code) {
            (Some(b), Some(c)) if c < b => (c, "`"),
            (Some(b), _) => (b, "**"),
            (None, Some(c)) => (c, "`"),
            (None, None) => break,
        };
        let after = &rest[pos + marker.len()..];
        let Some(end) = after.find(marker) else {
            break;
        };
        if pos > 0 {
            spans.push(Span::styled(rest[..pos].to_string(), base));
        }
        let inner = after[..end].to_string();
        spans.push(if marker == "`" {
            Span::styled(inner, base.fg(ACCENT))
        } else {
            Span::styled(inner, base.add_modifier(Modifier::BOLD))
        });
        rest = &after[end + marker.len()..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), base));
    }
    spans
}

fn markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(Line::from(Span::styled(
                format!("  {raw}"),
                Style::new().fg(ACCENT),
            )));
        } else if let Some(h) = raw
            .strip_prefix("### ")
            .or_else(|| raw.strip_prefix("## "))
            .or_else(|| raw.strip_prefix("# "))
        {
            out.push(Line::from(Span::styled(
                h.to_string(),
                Style::new().add_modifier(Modifier::BOLD),
            )));
        } else if let Some(item) = raw.strip_prefix("- ").or_else(|| raw.strip_prefix("* ")) {
            let mut spans = vec![Span::styled("  • ", Style::new().fg(ACCENT))];
            spans.extend(inline_spans(item, Style::new()));
            out.push(Line::from(spans));
        } else {
            out.push(Line::from(inline_spans(raw, Style::new())));
        }
    }
    out
}

fn transcript(m: &Model) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for item in &m.items {
        match item {
            Item::User(t) => {
                lines.push(Line::from(""));
                for (i, l) in t.lines().enumerate() {
                    let prefix = if i == 0 { "❯ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::new().fg(Color::Green).bold()),
                        Span::styled(l.to_string(), Style::new().bold()),
                    ]));
                }
                lines.push(Line::from(""));
            }
            Item::Bot(t) => {
                lines.extend(markdown_lines(t));
            }
            Item::Tool {
                name,
                summary,
                detail,
                status,
                preview,
            } => {
                let (icon, color) = match status {
                    ToolStatus::Running => (SPINNER[m.frame % SPINNER.len()], ACCENT),
                    ToolStatus::Ok => ("✓", Color::Green),
                    ToolStatus::Err => ("✗", Color::Red),
                    ToolStatus::Denied => ("⊘", Color::Yellow),
                };
                let mut head = vec![
                    Span::styled(format!("{icon} "), Style::new().fg(color)),
                    Span::styled(name.clone(), Style::new().bold()),
                    Span::styled(format!(" {summary}"), Style::new().fg(MUTED)),
                ];
                if *status == ToolStatus::Denied {
                    head.push(Span::styled(" — denied", Style::new().fg(Color::Yellow)));
                }
                lines.push(Line::from(head));
                if name == "edit_file" || name == "write_file" {
                    let total = detail.lines().count();
                    for l in detail.lines().take(DIFF_LINES) {
                        let style = if l.starts_with("+ ") {
                            Style::new().fg(Color::Green)
                        } else if l.starts_with("- ") {
                            Style::new().fg(Color::Red)
                        } else {
                            Style::new().fg(MUTED)
                        };
                        lines.push(Line::from(Span::styled(format!("  │ {l}"), style)));
                    }
                    if total > DIFF_LINES {
                        lines.push(Line::from(Span::styled(
                            format!("  │ … {} more lines", total - DIFF_LINES),
                            Style::new().fg(MUTED),
                        )));
                    }
                } else if name == "shell" && !detail.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("  │ {detail}"),
                        Style::new().fg(MUTED),
                    )));
                }
                let pcolor = if *status == ToolStatus::Err {
                    Color::Red
                } else {
                    MUTED
                };
                for l in preview.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  │ {l}"),
                        Style::new().fg(pcolor),
                    )));
                }
            }
            Item::Note(t, c) => {
                lines.push(Line::from(Span::styled(t.clone(), Style::new().fg(*c))))
            }
            Item::Info(t) => {
                for l in t.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {l}"),
                        Style::new().fg(ACCENT),
                    )));
                }
            }
            Item::End(t, c) => {
                lines.push(Line::from(Span::styled(
                    format!("  {t}"),
                    Style::new().fg(*c).add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  What should we work on?",
            Style::new().bold(),
        )));
        lines.push(Line::from(Span::styled(
            "  Ask anything, or type / for commands and skills (e.g. /code-review, /deep-research).",
            Style::new().fg(MUTED),
        )));
    }
    Text::from(lines)
}

/// Input text split into rows of at most `width` columns, plus the cursor's row/column.
fn wrap_input(chars: &[char], cursor: usize, width: usize) -> (Vec<String>, (usize, usize)) {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut col = 0;
    let mut pos = (0, 0);
    for (i, c) in chars.iter().enumerate() {
        if i == cursor {
            pos = (rows.len() - 1, col);
        }
        if *c == '\n' {
            rows.push(String::new());
            col = 0;
            continue;
        }
        let w = Span::raw(c.to_string()).width();
        if col + w > width {
            rows.push(String::new());
            col = 0;
        }
        rows.last_mut().unwrap().push(*c);
        col += w;
    }
    if cursor >= chars.len() {
        if col >= width {
            rows.push(String::new());
            col = 0;
        }
        pos = (rows.len() - 1, col);
    }
    (rows, pos)
}

pub fn draw(f: &mut Frame, m: &mut Model) {
    let area = f.area();
    let input_width = area.width.saturating_sub(4) as usize;
    let (rows, (cur_row, cur_col)) = wrap_input(&m.input, m.cursor, input_width);
    let input_height = (rows.len().clamp(1, 6) + 2) as u16;
    let suggestions = m.suggestions();
    let hint_height = u16::from(!suggestions.is_empty());
    let [top, body, hint, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(hint_height),
        Constraint::Length(input_height),
    ])
    .areas(area);

    // Top bar.
    let status = if m.busy {
        let secs = m.busy_since.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        Span::styled(
            format!(
                "{} working {secs}s · Esc to stop ",
                SPINNER[m.frame % SPINNER.len()]
            ),
            Style::new().fg(ACCENT),
        )
    } else {
        Span::styled("● ready ", Style::new().fg(Color::Green))
    };
    let (root, approval) = m
        .state
        .as_ref()
        .map(|s| (s.root.clone(), s.approval.clone()))
        .unwrap_or_default();
    let model = if m.model_label.is_empty() {
        "no model yet".to_string()
    } else {
        m.model_label.clone()
    };
    let right = vec![
        Span::styled(format!("{model} "), Style::new().fg(MUTED)),
        Span::styled(format!("· {approval} "), Style::new().fg(MUTED)),
        status,
    ];
    let right_width: usize = right.iter().map(|s| s.width()).sum();
    let left_room = (top.width as usize).saturating_sub(right_width + 14);
    let root: String = if root.chars().count() > left_room {
        format!(
            "…{}",
            root.chars()
                .skip(root.chars().count() - left_room.saturating_sub(1))
                .collect::<String>()
        )
    } else {
        root
    };
    let pad = (top.width as usize).saturating_sub(right_width + 12 + root.chars().count());
    let mut bar = vec![
        Span::styled(" ◐ Oblivion ", Style::new().fg(ACCENT).bold()),
        Span::styled(root, Style::new().fg(MUTED)),
        Span::raw(" ".repeat(pad)),
    ];
    bar.extend(right);
    f.render_widget(Paragraph::new(Line::from(bar)), top);

    // Conversation and side panel.
    let wide = body.width >= 100;
    let (chat_area, side_area) = if wide {
        let [c, s] = Layout::horizontal([Constraint::Min(40), Constraint::Length(SIDEBAR_WIDTH)])
            .areas(body);
        (c, Some(s))
    } else {
        (body, None)
    };
    let chat_inner = Rect {
        x: chat_area.x + 1,
        width: chat_area.width.saturating_sub(2),
        ..chat_area
    };
    let para = Paragraph::new(transcript(m)).wrap(Wrap { trim: false });
    let total = para.line_count(chat_inner.width);
    let height = chat_inner.height as usize;
    let max_scroll = total.saturating_sub(height);
    m.scroll = m.scroll.min(max_scroll);
    let top_line = max_scroll - m.scroll;
    f.render_widget(
        para.scroll((top_line.min(u16::MAX as usize) as u16, 0)),
        chat_inner,
    );
    if m.scroll > 0 {
        let tag = format!(" ↓ {} more lines (PgDn) ", m.scroll);
        let w = tag.chars().count() as u16;
        let r = Rect {
            x: chat_inner.x + chat_inner.width.saturating_sub(w),
            y: chat_inner.y + chat_inner.height.saturating_sub(1),
            width: w.min(chat_inner.width),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled(tag, Style::new().fg(Color::Black).bg(ACCENT))),
            r,
        );
    }
    if let Some(side) = side_area {
        draw_side(f, m, side);
    }

    // Slash command suggestions.
    if !suggestions.is_empty() {
        let mut spans = vec![Span::styled(" Tab ▸ ", Style::new().fg(MUTED))];
        for (i, s) in suggestions.iter().enumerate() {
            let style = if i == 0 {
                Style::new().fg(ACCENT).bold()
            } else {
                Style::new().fg(MUTED)
            };
            spans.push(Span::styled(format!("/{s}  "), style));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), hint);
    }

    // Input box.
    let title = if m.busy {
        " working… type your next message (it runs after this one) "
    } else {
        " Ask Oblivion · Enter send · Shift+Enter new line · PgUp/PgDn scroll · Ctrl+C quit "
    };
    let border = if m.approvals.is_empty() {
        ACCENT
    } else {
        MUTED
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border))
        .title(Span::styled(title, Style::new().fg(MUTED)));
    let visible = (input_height - 2) as usize;
    let first_row = (cur_row + 1).saturating_sub(visible);
    let shown: Vec<Line> = rows
        .iter()
        .skip(first_row)
        .take(visible)
        .map(|r| Line::from(r.clone()))
        .collect();
    let placeholder = m.input.is_empty();
    let text = if placeholder {
        Text::from(Span::styled(
            "Type a request, or / for commands and skills",
            Style::new().fg(MUTED),
        ))
    } else {
        Text::from(shown)
    };
    f.render_widget(Paragraph::new(text).block(block), input_area);
    if m.approvals.is_empty() && area.width > 0 && area.height > 0 {
        let max_x = area.width.saturating_sub(1);
        let max_y = area.height.saturating_sub(1);
        f.set_cursor_position(Position {
            x: (input_area.x + 1 + cur_col as u16).min(max_x),
            y: (input_area.y + 1 + (cur_row - first_row) as u16).min(max_y),
        });
    }

    // Approval pop-up.
    if let Some(a) = m.approvals.front() {
        let w = (chat_area.width.saturating_sub(6)).clamp(30, 90);
        let summary_rows = (a.summary.chars().count() as u16 / w.saturating_sub(4).max(1)) + 1;
        let h = 6 + summary_rows;
        let r = Rect {
            x: chat_area.x + (chat_area.width.saturating_sub(w)) / 2,
            y: chat_area.y + chat_area.height.saturating_sub(h + 1),
            width: w.min(chat_area.width),
            height: h.min(chat_area.height),
        };
        f.render_widget(Clear, r);
        let more = if m.approvals.len() > 1 {
            format!(" (+{} more waiting)", m.approvals.len() - 1)
        } else {
            String::new()
        };
        let body = Text::from(vec![
            Line::from(Span::styled(
                format!("Allow {}?", a.summary),
                Style::new().bold(),
            )),
            Line::from(Span::styled(
                format!("{} wants to do this{more}", a.tool),
                Style::new().fg(MUTED),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(" y ", Style::new().fg(Color::Black).bg(Color::Green).bold()),
                Span::raw(" allow once   "),
                Span::styled(" a ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
                Span::raw(format!(" always allow {}   ", a.tool)),
                Span::styled(" n ", Style::new().fg(Color::Black).bg(Color::Red).bold()),
                Span::raw(" deny"),
            ]),
        ]);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Yellow))
            .title(Span::styled(
                " Approval needed ",
                Style::new().fg(Color::Yellow).bold(),
            ));
        f.render_widget(
            Paragraph::new(body).wrap(Wrap { trim: false }).block(block),
            r,
        );
    }
}

fn draw_side(f: &mut Frame, m: &Model, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let heading = |t: &str| Line::from(Span::styled(t.to_string(), Style::new().fg(MUTED).bold()));
    lines.push(heading("TASKS"));
    if m.todos.is_empty() {
        lines.push(Line::from(Span::styled(
            "No task list yet.",
            Style::new().fg(MUTED),
        )));
    }
    for t in &m.todos {
        let (mark, style) = match t.status.as_str() {
            "completed" => (
                "✓ ",
                Style::new().fg(MUTED).add_modifier(Modifier::CROSSED_OUT),
            ),
            "in_progress" => ("▸ ", Style::new().fg(ACCENT).bold()),
            _ => ("○ ", Style::new()),
        };
        lines.push(Line::from(vec![
            Span::styled(mark, style),
            Span::styled(t.text.clone(), style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(heading("HELPERS"));
    if m.lanes.is_empty() {
        lines.push(Line::from(Span::styled(
            "None running.",
            Style::new().fg(MUTED),
        )));
    }
    for (i, lane) in &m.lanes {
        let color = if lane.done { Color::Green } else { ACCENT };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", i + 1), Style::new().fg(color)),
            Span::styled(lane.label.clone(), Style::new().bold()),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  {}", lane.last),
            Style::new().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    lines.push(heading("MODELS"));
    if let Some(s) = &m.state {
        for md in &s.models {
            let current = md.label == m.model_label;
            let usage = match (md.cooldown_secs, md.used_today, md.per_day) {
                (Some(c), _, _) => format!("cooling {c}s"),
                (None, Some(u), Some(d)) => format!("{u}/{d}"),
                (None, Some(u), None) => u.to_string(),
                _ => String::new(),
            };
            let style = if md.cooldown_secs.is_some() {
                Style::new().fg(Color::Yellow)
            } else if current {
                Style::new().fg(ACCENT)
            } else {
                Style::new()
            };
            lines.push(Line::from(vec![
                Span::styled(if current { "● " } else { "  " }, Style::new().fg(ACCENT)),
                Span::styled(md.label.clone(), style),
            ]));
            if !usage.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    {usage} today"),
                    Style::new().fg(MUTED),
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("session {}", s.session),
            Style::new().fg(MUTED),
        )));
    }
    let block = Block::new()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(MUTED));
    f.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(block.padding(ratatui::widgets::Padding::left(1))),
        area,
    );
}

// ---------- running ----------

pub async fn run(
    worker: Worker,
    controls: Controls,
    events: UnboundedReceiver<UiEvent>,
) -> Result<()> {
    let terminal = ratatui::try_init()?;
    let _ = crossterm::execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let ui = tokio::spawn(ui_loop(terminal, controls.clone(), events));
    let work = worker.run();
    tokio::pin!(work);
    tokio::pin!(ui);
    let result = tokio::select! {
        _ = &mut work => ui.await.map_err(anyhow::Error::from).and_then(|r| r),
        r = &mut ui => r.map_err(anyhow::Error::from).and_then(|r| r),
    };
    // Release an approval that may still be waiting, then give the terminal back.
    controls.cancel();
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
    ratatui::restore();
    let _ = std::io::stdout().flush();
    result
}

async fn ui_loop(
    mut terminal: ratatui::DefaultTerminal,
    controls: Controls,
    mut events: UnboundedReceiver<UiEvent>,
) -> Result<()> {
    let mut model = Model::default();
    let mut keys = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    loop {
        terminal.draw(|f| draw(f, &mut model))?;
        tokio::select! {
            ev = keys.next() => match ev {
                Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => {
                    if let Some(text) = model.key(k, Some(&controls)) {
                        controls.send(text);
                    }
                }
                Some(Ok(Event::Paste(p))) => model.paste(&p),
                Some(Ok(Event::Mouse(m))) => match m.kind {
                    MouseEventKind::ScrollUp => model.scroll += 3,
                    MouseEventKind::ScrollDown => model.scroll = model.scroll.saturating_sub(3),
                    _ => {}
                },
                Some(Ok(Event::Resize(_, _))) => {
                    let _ = terminal.clear();
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => break,
            },
            e = events.recv() => match e {
                Some(e) => {
                    model.apply(e);
                    // Streamed text arrives in many small pieces; take all that are waiting.
                    while let Ok(e) = events.try_recv() {
                        model.apply(e);
                    }
                }
                None => break,
            },
            _ = tick.tick() => {
                model.frame = model.frame.wrapping_add(1);
            }
        }
        if model.quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ModelView;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn screen(m: &mut Model, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, m)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn sample() -> Model {
        let mut m = Model::default();
        m.apply(UiEvent::State(StateView {
            version: "0.1.0".into(),
            root: "D:\\Jarvis\\oblivion".into(),
            session: "20261115-1".into(),
            approval: "ask".into(),
            models: vec![ModelView {
                label: "groq/gpt-oss-120b".into(),
                cooldown_secs: None,
                used_today: Some(41),
                per_day: Some(1000),
            }],
            todos: vec![],
            skills: vec!["code-review".into(), "deep-research".into()],
            browser: false,
            instructions_file: Some("AGENTS.md".into()),
            spoken_reply: false,
            wake_word_enabled: false,
        }));
        m.apply(UiEvent::User {
            text: "add a version flag".into(),
        });
        m.apply(UiEvent::Busy { busy: true });
        m.apply(UiEvent::Model {
            label: "groq/gpt-oss-120b".into(),
        });
        m.apply(UiEvent::Text {
            text: "I will edit **main.rs** ".into(),
        });
        m.apply(UiEvent::Text {
            text: "now.".into(),
        });
        m.apply(UiEvent::ToolStart {
            name: "edit_file".into(),
            summary: "edit src/main.rs".into(),
            detail: "- struct Cli {\n+ #[command(version)]\n+ struct Cli {".into(),
        });
        m.apply(UiEvent::Todos {
            items: vec![TodoView {
                text: "Add the flag".into(),
                status: "in_progress".into(),
            }],
        });
        m.apply(UiEvent::Sub {
            index: 0,
            label: "Check Codex docs".into(),
            event: Box::new(UiEvent::ToolStart {
                name: "web_fetch".into(),
                summary: "codex README".into(),
                detail: String::new(),
            }),
        });
        m
    }

    #[test]
    fn streamed_text_joins_and_tools_update() {
        let mut m = sample();
        assert_eq!(m.items[1], Item::Bot("I will edit **main.rs** now.".into()));
        m.apply(UiEvent::ToolEnd {
            name: "edit_file".into(),
            preview: "Edited src/main.rs".into(),
            is_error: false,
            output: String::new(),
        });
        assert!(matches!(
            &m.items[2],
            Item::Tool {
                status: ToolStatus::Ok,
                ..
            }
        ));
        m.apply(UiEvent::ToolStart {
            name: "shell".into(),
            summary: "cargo test".into(),
            detail: "$ cargo test".into(),
        });
        m.apply(UiEvent::Declined {
            name: "shell".into(),
        });
        assert!(matches!(
            &m.items[3],
            Item::Tool {
                status: ToolStatus::Denied,
                ..
            }
        ));
        m.apply(UiEvent::TurnEnd {
            steps: 2,
            tool_calls: 2,
            prompt_tokens: 900,
            completion_tokens: 80,
            outcome: "done".into(),
            error: None,
        });
        assert!(matches!(m.items.last(), Some(Item::End(t, _)) if t.contains("2 step(s)")));
    }

    #[test]
    fn screen_shows_conversation_side_panel_and_diff() {
        let mut m = sample();
        let s = screen(&mut m, 120, 30);
        assert!(s.contains("◐ Oblivion"), "{s}");
        assert!(s.contains("❯ add a version flag"));
        assert!(
            s.contains("I will edit main.rs now."),
            "bold markers are hidden:\n{s}"
        );
        assert!(s.contains("edit_file edit src/main.rs"));
        assert!(s.contains("│ + #[command(version)]"));
        assert!(s.contains("TASKS") && s.contains("▸ Add the flag"));
        assert!(s.contains("HELPERS") && s.contains("1 Check Codex docs"));
        assert!(s.contains("groq/gpt-oss-120b"));
        assert!(s.contains("working"));
        // Narrow terminals drop the side panel.
        let narrow = screen(&mut m, 80, 24);
        assert!(!narrow.contains("TASKS"));
    }

    #[test]
    fn approval_popup_takes_y_a_n() {
        let mut m = sample();
        m.apply(UiEvent::Approval {
            id: 7,
            tool: "edit_file".into(),
            summary: "edit src/main.rs".into(),
            low_risk: false,
        });
        let s = screen(&mut m, 120, 30);
        assert!(
            s.contains("Approval needed") && s.contains("Allow edit src/main.rs?"),
            "{s}"
        );
        // Typing goes nowhere while the pop-up is open; 'n' answers it.
        assert_eq!(m.key(key(KeyCode::Char('x')), None), None);
        assert!(m.input.is_empty());
        m.key(key(KeyCode::Char('n')), None);
        assert!(m.approvals.is_empty());
        assert!(!screen(&mut m, 120, 30).contains("Approval needed"));
    }

    #[test]
    fn typing_history_completion_and_newlines() {
        let mut m = sample();
        for c in "/dee".chars() {
            m.key(key(KeyCode::Char(c)), None);
        }
        assert_eq!(m.suggestions(), vec!["deep-research".to_string()]);
        assert!(screen(&mut m, 120, 30).contains("Tab ▸ /deep-research"));
        m.key(key(KeyCode::Tab), None);
        assert_eq!(m.input_text(), "/deep-research ");
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), None);
        m.key(key(KeyCode::Char('x')), None);
        assert_eq!(m.input_text(), "/deep-research \nx");
        assert_eq!(
            m.key(key(KeyCode::Enter), None).as_deref(),
            Some("/deep-research \nx")
        );
        assert!(m.input.is_empty());
        m.key(key(KeyCode::Up), None);
        assert_eq!(m.input_text(), "/deep-research \nx");
        m.set_input("hello");
        m.key(key(KeyCode::Left), None);
        m.key(key(KeyCode::Backspace), None);
        assert_eq!(m.input_text(), "helo");
        m.paste("a\r\nb");
        assert_eq!(m.input_text(), "hela\nbo");
    }

    #[test]
    fn long_input_wraps_and_scrolling_is_clamped() {
        let (rows, pos) = wrap_input(&"abcdefghij".chars().collect::<Vec<_>>(), 10, 4);
        assert_eq!(rows, ["abcd", "efgh", "ij"]);
        assert_eq!(pos, (2, 2));
        let mut m = sample();
        for i in 0..200 {
            m.apply(UiEvent::Info {
                text: format!("line {i}"),
            });
        }
        m.scroll = 10_000;
        let s = screen(&mut m, 120, 30);
        assert!(s.contains("more lines (PgDn)"));
        assert!(m.scroll < 10_000, "scroll is clamped to the content");
    }

    #[test]
    fn windows_terminal_paste_newlines_and_resize_resilience() {
        let mut m = sample();
        // Paste with lone \r and control chars
        m.paste("foo\rbar\x1b[31mbaz\0");
        assert_eq!(m.input_text(), "foo\nbarbaz");

        // Char('\n') with Shift inserts newline
        m.key(
            KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::SHIFT),
            None,
        );
        m.key(key(KeyCode::Char('z')), None);
        assert_eq!(m.input_text(), "foo\nbarbaz\nz");

        // Submit with Char('\r')
        let sent = m.key(key(KeyCode::Char('\r')), None);
        assert_eq!(sent.as_deref(), Some("foo\nbarbaz\nz"));
        assert!(m.input.is_empty());

        // Extreme resize: width 15, height 5 and 5, 2 should not panic
        let _ = screen(&mut m, 15, 5);
        let _ = screen(&mut m, 5, 2);

        // Approval with uppercase Y and Esc
        m.apply(UiEvent::Approval {
            id: 42,
            tool: "write_file".into(),
            summary: "write test.txt".into(),
            low_risk: false,
        });
        assert!(screen(&mut m, 120, 30).contains("Approval needed"));
        m.key(key(KeyCode::Char('Y')), None);
        assert!(m.approvals.is_empty());

        m.apply(UiEvent::Approval {
            id: 43,
            tool: "run_command".into(),
            summary: "cargo build".into(),
            low_risk: false,
        });
        m.key(key(KeyCode::Esc), None);
        assert!(m.approvals.is_empty());
    }
}
