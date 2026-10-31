// An arrow-key list in the terminal: groups with a heading each, one row per item,
// enter picks. Drawn in place under whatever was printed before it, so the status
// lines above stay on screen. Mirrors docs/frontends/cli.md#jazyk.
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::{cursor, execute, queue, terminal};
use std::io::Write;

pub struct Row {
    pub key: String,
    pub name: String,
    // Dim text after the name, e.g. `not installed`.
    pub note: String,
}

pub struct Group {
    pub heading: String,
    pub rows: Vec<Row>,
}

enum Line {
    Heading(usize),
    Item(usize, usize),
}

// The picked (group, row), or None when the person quits (q, esc, ctrl-c).
// `start` is the row the cursor begins on.
pub fn pick(groups: &[Group], start: (usize, usize)) -> Option<(usize, usize)> {
    let lines: Vec<Line> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, group)| {
            std::iter::once(Line::Heading(g))
                .chain((0..group.rows.len()).map(move |r| Line::Item(g, r)))
        })
        .collect();
    let items: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| matches!(l, Line::Item(..)))
        .map(|(i, _)| i)
        .collect();
    if items.is_empty() {
        return None;
    }
    let mut at = items
        .iter()
        .position(|&i| matches!(lines[i], Line::Item(g, r) if (g, r) == start))
        .unwrap_or(0);
    let key_width = groups
        .iter()
        .flat_map(|g| g.rows.iter().map(|r| r.key.chars().count()))
        .max()
        .unwrap_or(0);
    let name_width = groups
        .iter()
        .flat_map(|g| g.rows.iter().map(|r| r.name.chars().count()))
        .max()
        .unwrap_or(0);

    let mut out = std::io::stdout();
    if terminal::enable_raw_mode().is_err() {
        return None;
    }
    let _ = execute!(out, cursor::Hide);
    let mut drawn = 0u16;
    let picked = loop {
        // A list taller than the terminal scrolls: the window follows the cursor and
        // keeps the heading of the cursor's group when it fits.
        // A terminal that reports no size (0 rows) gets the classic 24.
        let rows = match terminal::size().map(|(_, h)| h as usize) {
            Ok(h) if h >= 4 => h,
            _ => 24,
        };
        let height = lines.len().min(rows - 2);
        let cursor_line = items[at];
        let mut top = cursor_line.saturating_sub(height.saturating_sub(1));
        if cursor_line > 0 && matches!(lines[cursor_line - 1], Line::Heading(_)) {
            top = top.min(cursor_line - 1);
        }
        top = top.min(lines.len() - height);
        if drawn > 0 {
            let _ = queue!(out, cursor::MoveUp(drawn));
        }
        let _ = queue!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::FromCursorDown));
        for (i, line) in lines.iter().enumerate().skip(top).take(height) {
            match line {
                Line::Heading(g) => {
                    let _ = queue!(
                        out,
                        SetAttribute(Attribute::Bold),
                        Print(&groups[*g].heading),
                        SetAttribute(Attribute::Reset),
                        Print("\r\n")
                    );
                }
                Line::Item(g, r) => {
                    let row = &groups[*g].rows[*r];
                    let here = i == cursor_line;
                    let text = format!(
                        "{} {:kw$}  {:nw$}",
                        if here { ">" } else { " " },
                        row.key,
                        row.name,
                        kw = key_width,
                        nw = name_width
                    );
                    if here {
                        let _ = queue!(out, SetAttribute(Attribute::Reverse), Print(&text), SetAttribute(Attribute::Reset));
                    } else {
                        let _ = queue!(out, Print(&text));
                    }
                    if !row.note.is_empty() {
                        let _ = queue!(
                            out,
                            Print("  "),
                            SetAttribute(Attribute::Dim),
                            Print(&row.note),
                            SetAttribute(Attribute::Reset)
                        );
                    }
                    let _ = queue!(out, Print("\r\n"));
                }
            }
        }
        let _ = queue!(
            out,
            SetAttribute(Attribute::Dim),
            Print("up/down move, enter start, q quit"),
            SetAttribute(Attribute::Reset)
        );
        let _ = out.flush();
        drawn = height as u16;
        let Ok(Event::Key(KeyEvent { code, modifiers, kind, .. })) = event::read() else {
            continue;
        };
        if kind == KeyEventKind::Release {
            continue;
        }
        match code {
            KeyCode::Up | KeyCode::BackTab => at = (at + items.len() - 1) % items.len(),
            KeyCode::Down | KeyCode::Tab => at = (at + 1) % items.len(),
            KeyCode::Char('k') if modifiers.is_empty() => at = (at + items.len() - 1) % items.len(),
            KeyCode::Char('j') if modifiers.is_empty() => at = (at + 1) % items.len(),
            KeyCode::Home => at = 0,
            KeyCode::End => at = items.len() - 1,
            KeyCode::Enter => break Some(at),
            KeyCode::Esc | KeyCode::Char('q') => break None,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break None,
            // A letter jumps to the next row whose key starts with it.
            KeyCode::Char(c) => {
                let n = items.len();
                if let Some(next) = (1..=n).map(|d| (at + d) % n).find(|&j| match lines[items[j]] {
                    Line::Item(g, r) => groups[g].rows[r].key.starts_with(c),
                    _ => false,
                }) {
                    at = next;
                }
            }
            _ => {}
        }
    };
    // Leave the list on screen with the pick marked, and the cursor below it.
    let _ = execute!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::CurrentLine), Print("\r\n"), cursor::Show);
    let _ = terminal::disable_raw_mode();
    picked.map(|i| match lines[items[i]] {
        Line::Item(g, r) => (g, r),
        Line::Heading(_) => unreachable!(),
    })
}
