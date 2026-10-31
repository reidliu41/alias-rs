use std::io;

use anyhow::{Context, Result};
use chrono::Utc;
use console::{Key, Term, style};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use unicode_width::UnicodeWidthChar;

use crate::history::HistoryEntry;
use crate::model::Alias;

pub enum Selection {
    Execute(String),
    Insert(String),
    Cancel,
}

pub fn run(aliases: &[Alias]) -> Result<Selection> {
    if aliases.is_empty() {
        eprintln!("No aliases found.");
        return Ok(Selection::Cancel);
    }

    let mut screen = Screen::new()?;
    let matcher = SkimMatcherV2::default();
    let mut query = String::new();
    let mut selected = 0_usize;

    loop {
        let matches = filtered_aliases(aliases, &matcher, &query);
        if selected >= matches.len() {
            selected = 0;
        }
        screen.draw(aliases, &matches, selected, &query)?;

        match screen.read_key()? {
            Key::ArrowUp if !matches.is_empty() => {
                selected = if selected == 0 {
                    matches.len() - 1
                } else {
                    selected - 1
                };
            }
            Key::ArrowDown if !matches.is_empty() => {
                selected = (selected + 1) % matches.len();
            }
            Key::PageUp if !matches.is_empty() => {
                selected = selected.saturating_sub(screen.visible_rows());
            }
            Key::PageDown if !matches.is_empty() => {
                selected = (selected + screen.visible_rows()).min(matches.len() - 1);
            }
            Key::Enter if !matches.is_empty() => {
                return Ok(Selection::Execute(aliases[matches[selected]].cmd.clone()));
            }
            Key::ArrowRight if !matches.is_empty() => {
                return Ok(Selection::Insert(aliases[matches[selected]].cmd.clone()));
            }
            Key::ArrowLeft | Key::Escape | Key::CtrlC => return Ok(Selection::Cancel),
            Key::Backspace => {
                query.pop();
                selected = 0;
            }
            Key::Char(character) if !character.is_control() => {
                query.push(character);
                selected = 0;
            }
            _ => {}
        }
    }
}

pub fn run_history(entries: &[HistoryEntry]) -> Result<Option<HistoryEntry>> {
    if entries.is_empty() {
        eprintln!("No usable shell history found.");
        return Ok(None);
    }

    let mut screen = Screen::new()?;
    let matcher = SkimMatcherV2::default();
    let mut query = String::new();
    let mut selected = entries.len() - 1;

    loop {
        let matches = filtered_history(entries, &matcher, &query);
        if selected >= matches.len() {
            selected = matches.len().saturating_sub(1);
        }
        screen.draw_history(entries, &matches, selected, &query)?;

        match screen.read_key()? {
            Key::ArrowUp if !matches.is_empty() => {
                selected = if selected == 0 {
                    matches.len() - 1
                } else {
                    selected - 1
                };
            }
            Key::ArrowDown if !matches.is_empty() => {
                selected = (selected + 1) % matches.len();
            }
            Key::PageUp if !matches.is_empty() => {
                selected = selected.saturating_sub(screen.visible_rows());
            }
            Key::PageDown if !matches.is_empty() => {
                selected = (selected + screen.visible_rows()).min(matches.len() - 1);
            }
            Key::Enter if !matches.is_empty() => {
                return Ok(Some(entries[matches[selected]].clone()));
            }
            Key::ArrowLeft | Key::Escape | Key::CtrlC => return Ok(None),
            Key::Backspace => {
                query.pop();
                selected = filtered_history(entries, &matcher, &query)
                    .len()
                    .saturating_sub(1);
            }
            Key::Char(character) if !character.is_control() => {
                query.push(character);
                selected = filtered_history(entries, &matcher, &query)
                    .len()
                    .saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn filtered_aliases(aliases: &[Alias], matcher: &SkimMatcherV2, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..aliases.len()).collect();
    }

    let mut matches = aliases
        .iter()
        .enumerate()
        .filter_map(|(index, alias)| {
            let searchable = format!(
                "{} {} {} {} {}",
                alias.created_at_display(),
                alias.name,
                alias.cmd,
                alias.desc.as_deref().unwrap_or_default(),
                alias.tags.join(" ")
            );
            matcher
                .fuzzy_match(&searchable, query)
                .map(|score| (index, score))
        })
        .collect::<Vec<_>>();
    matches.sort_unstable_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_index.cmp(right_index))
    });
    matches.into_iter().map(|(index, _)| index).collect()
}

fn filtered_history(entries: &[HistoryEntry], matcher: &SkimMatcherV2, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..entries.len()).rev().collect();
    }

    let mut matches = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            matcher
                .fuzzy_match(&entry.command, query)
                .map(|score| (index, score))
        })
        .collect::<Vec<_>>();
    matches.sort_unstable_by(|(left_index, left_score), (right_index, right_score)| {
        left_score
            .cmp(right_score)
            .then_with(|| right_index.cmp(left_index))
    });
    matches.into_iter().map(|(index, _)| index).collect()
}

struct Screen {
    term: Term,
    rendered_lines: usize,
}

impl Screen {
    fn new() -> Result<Self> {
        let term = Term::stderr();
        term.hide_cursor().context("could not hide the cursor")?;
        Ok(Self {
            term,
            rendered_lines: 0,
        })
    }

    fn visible_rows(&self) -> usize {
        (self.term.size().0 as usize).saturating_sub(3).max(1)
    }

    fn draw(
        &mut self,
        aliases: &[Alias],
        matches: &[usize],
        selected: usize,
        query: &str,
    ) -> Result<()> {
        self.clear()?;
        let width = (self.term.size().1 as usize).max(20);
        let visible_rows = self.visible_rows();
        let start = if selected >= visible_rows {
            selected + 1 - visible_rows
        } else {
            0
        };

        let help = fit("↑/↓ move  Enter run  → input  ←/Esc exit", width);
        self.term
            .write_line(&style(help).dim().to_string())
            .context("could not draw selector help")?;
        self.rendered_lines += 1;

        if matches.is_empty() {
            self.term
                .write_line(&style("  No matches").dim().to_string())
                .context("could not draw empty search result")?;
            self.rendered_lines += 1;
        } else {
            for (position, alias_index) in matches.iter().enumerate().skip(start).take(visible_rows)
            {
                let alias = &aliases[*alias_index];
                let details = alias_details(alias);
                let prefix = if position == selected { "❯ " } else { "  " };
                let line = format!("{prefix}{}", fit(&details, width.saturating_sub(2)));
                let line = if position == selected {
                    style(line).green().bold().to_string()
                } else {
                    line
                };
                self.term
                    .write_line(&line)
                    .context("could not draw alias")?;
                self.rendered_lines += 1;
            }
        }

        let search = fit(&format!("Search: {query}_"), width);
        self.term
            .write_line(&style(search).cyan().to_string())
            .context("could not draw search input")?;
        self.rendered_lines += 1;
        self.term.flush().context("could not flush selector")?;
        Ok(())
    }

    fn draw_history(
        &mut self,
        entries: &[HistoryEntry],
        matches: &[usize],
        selected: usize,
        query: &str,
    ) -> Result<()> {
        self.clear()?;
        let width = (self.term.size().1 as usize).max(20);
        let visible_rows = self.visible_rows();
        let start = if selected >= visible_rows {
            selected + 1 - visible_rows
        } else {
            0
        };
        let number_width = entries.len().to_string().len();
        let now = Utc::now().timestamp();

        let help = fit("↑/↓ move  Enter add  ←/Esc exit", width);
        self.term
            .write_line(&style(help).dim().to_string())
            .context("could not draw history help")?;
        self.rendered_lines += 1;

        if matches.is_empty() {
            self.term
                .write_line(&style("  No matches").dim().to_string())
                .context("could not draw empty history result")?;
            self.rendered_lines += 1;
        } else {
            for (position, entry_index) in matches.iter().enumerate().skip(start).take(visible_rows)
            {
                let details = format!(
                    "{:>8}  {:>number_width$}  {}",
                    relative_time(entries[*entry_index].timestamp, now),
                    entry_index + 1,
                    entries[*entry_index].command
                );
                let prefix = if position == selected { "❯ " } else { "  " };
                let line = format!("{prefix}{}", fit(&details, width.saturating_sub(2)));
                let line = if position == selected {
                    style(line).green().bold().to_string()
                } else {
                    line
                };
                self.term
                    .write_line(&line)
                    .context("could not draw history entry")?;
                self.rendered_lines += 1;
            }
        }

        let search = fit(&format!("Search: {query}_"), width);
        self.term
            .write_line(&style(search).cyan().to_string())
            .context("could not draw history search")?;
        self.rendered_lines += 1;
        self.term
            .flush()
            .context("could not flush history selector")?;
        Ok(())
    }

    fn read_key(&self) -> Result<Key> {
        self.term.read_key().context("could not read selector key")
    }

    fn clear(&mut self) -> io::Result<()> {
        if self.rendered_lines > 0 {
            self.term.clear_last_lines(self.rendered_lines)?;
            self.rendered_lines = 0;
        }
        Ok(())
    }
}

fn alias_details(alias: &Alias) -> String {
    let mut details = format!(
        "{:<16}  {}  {}",
        alias.created_at_display(),
        alias.name,
        alias.cmd
    );
    if let Some(desc) = &alias.desc {
        details.push_str("  # ");
        details.push_str(desc);
    }
    if !alias.tags.is_empty() {
        details.push_str("  [");
        details.push_str(&alias.tags.join(", "));
        details.push(']');
    }
    details
}

fn relative_time(timestamp: Option<i64>, now: i64) -> String {
    let Some(timestamp) = timestamp else {
        return "-".to_owned();
    };
    let seconds = now.saturating_sub(timestamp).max(0);
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        86_400..=2_591_999 => format!("{}d ago", seconds / 86_400),
        2_592_000..=31_535_999 => format!("{}mo ago", seconds / 2_592_000),
        _ => format!("{}y ago", seconds / 31_536_000),
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.clear();
        let _ = self.term.show_cursor();
        let _ = self.term.flush();
    }
}

fn fit(value: &str, max_width: usize) -> String {
    let width = value
        .chars()
        .filter_map(UnicodeWidthChar::width)
        .sum::<usize>();
    if width <= max_width {
        return value.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }

    let content_width = max_width.saturating_sub(1);
    let mut output = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > content_width {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_all_alias_fields() {
        let mut first = Alias::new(
            "gs",
            "git status",
            Some("Working tree".into()),
            vec!["git".into()],
        )
        .unwrap();
        first.created_at =
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-30T09:24:17-07:00").unwrap());
        let aliases = vec![
            first,
            Alias::new("ll", "ls -lah", None, vec!["files".into()]).unwrap(),
        ];
        let matcher = SkimMatcherV2::default();

        assert_eq!(filtered_aliases(&aliases, &matcher, "work"), [0]);
        assert_eq!(filtered_aliases(&aliases, &matcher, "files"), [1]);
        assert_eq!(filtered_aliases(&aliases, &matcher, "lah"), [1]);
        assert_eq!(filtered_aliases(&aliases, &matcher, "2026-10-30"), [0]);
    }

    #[test]
    fn truncates_without_splitting_unicode() {
        assert_eq!(fit("查看目录内容", 5), "查看…");
        assert_eq!(fit("abcdef", 4), "abc…");
    }

    #[test]
    fn displays_creation_time_in_fixed_width_column() {
        let mut alias = Alias::new("lzz", "ls -lah", None, vec![]).unwrap();
        alias.created_at =
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-30T09:24:17-07:00").unwrap());
        assert_eq!(alias_details(&alias), "2026-10-30 09:24  lzz  ls -lah");

        let legacy = Alias::new("lz", "ls -lah", None, vec![]).unwrap();
        assert_eq!(alias_details(&legacy), "-                 lz  ls -lah");
    }

    #[test]
    fn searches_history_without_changing_original_indices() {
        let entries = vec![
            history_entry("cargo test", 3),
            history_entry("git status", 2),
            history_entry("cargo clippy", 1),
        ];
        let matcher = SkimMatcherV2::default();
        assert_eq!(filtered_history(&entries, &matcher, ""), [2, 1, 0]);
        let matches = filtered_history(&entries, &matcher, "git");
        assert_eq!(matches, [1]);
    }

    #[test]
    fn formats_history_times() {
        let now = 1_800_000_000;
        assert_eq!(relative_time(Some(now - 42), now), "42s ago");
        assert_eq!(relative_time(Some(now - 120), now), "2m ago");
        assert_eq!(relative_time(Some(now - 10_800), now), "3h ago");
        assert_eq!(relative_time(Some(now - 172_800), now), "2d ago");
        assert_eq!(relative_time(None, now), "-");
    }

    fn history_entry(command: &str, timestamp: i64) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            timestamp: Some(timestamp),
        }
    }
}
