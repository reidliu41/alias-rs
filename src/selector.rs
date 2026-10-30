use std::io;

use anyhow::{Context, Result};
use console::{Key, Term, style};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use unicode_width::UnicodeWidthChar;

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
}
