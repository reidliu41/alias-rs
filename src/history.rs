use std::collections::HashSet;
use std::io::{self, Read};
use std::num::NonZeroUsize;

use anyhow::{Context, Result, bail};
use dialoguer::{Input, theme::ColorfulTheme};

use crate::model::Alias;
use crate::selector;
use crate::store::Store;
use crate::table;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub command: String,
    pub timestamp: Option<i64>,
}

pub fn read_from_stdin(shell: &str) -> Result<Vec<HistoryEntry>> {
    let mut input = Vec::new();
    io::stdin()
        .read_to_end(&mut input)
        .context("could not read shell history")?;

    let entries = match shell {
        "fish" => input
            .split(|byte| *byte == 0)
            .map(String::from_utf8_lossy)
            .map(|entry| parse_timestamped(entry.as_ref()))
            .collect(),
        "bash" => parse_bash(&String::from_utf8_lossy(&input)),
        "zsh" => String::from_utf8_lossy(&input)
            .lines()
            .map(parse_timestamped)
            .collect(),
        _ => bail!("unsupported history shell `{shell}`; supported shells: fish, bash, zsh"),
    };

    Ok(normalize(entries))
}

pub fn select_and_add(store: &Store, entries: &[HistoryEntry]) -> Result<Option<Alias>> {
    let Some(command) = selector::run_history(entries)? else {
        return Ok(None);
    };
    add_command(store, command.command, None, None, Vec::new()).map(Some)
}

pub fn add_last(
    store: &Store,
    entries: &[HistoryEntry],
    index: NonZeroUsize,
    name: Option<String>,
    desc: Option<String>,
    tags: Vec<String>,
) -> Result<Alias> {
    let position = index.get() - 1;
    let command = entries.get(position).ok_or_else(|| {
        anyhow::anyhow!(
            "requested history item {}, but only {} usable entries exist; run `als h` to browse them",
            index,
            entries.len()
        )
    })?;
    add_command(store, command.command.clone(), name, desc, tags)
}

fn add_command(
    store: &Store,
    command: String,
    name: Option<String>,
    desc: Option<String>,
    tags: Vec<String>,
) -> Result<Alias> {
    if name.is_none() {
        eprintln!("Command: {command}");
    }
    let name = match name {
        Some(name) => name,
        None => Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Alias")
            .interact_text()
            .context("could not read alias name")?,
    };

    let alias = Alias::new(name, command, desc, tags)?;
    store.add(alias.clone())?;
    print!("{}", table::added(&alias));
    Ok(alias)
}

fn parse_timestamped(value: &str) -> HistoryEntry {
    let value = value.trim_start();
    let (timestamp, command) = value
        .split_once('|')
        .and_then(|(timestamp, command)| {
            timestamp
                .parse::<i64>()
                .ok()
                .map(|timestamp| (Some(timestamp), command))
        })
        .unwrap_or((None, value));
    HistoryEntry {
        command: command.to_owned(),
        timestamp,
    }
}

fn parse_bash(value: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut current: Option<HistoryEntry> = None;

    for line in value.lines() {
        if let Some(payload) = strip_bash_history_number(line) {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            let payload = payload.strip_prefix("??").unwrap_or(payload);
            current = Some(parse_timestamped(payload));
        } else if let Some(entry) = &mut current {
            entry.command.push('\n');
            entry.command.push_str(line);
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    entries.reverse();
    entries
}

fn strip_bash_history_number(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    value.get(digits..)?.strip_prefix("  ")
}

fn normalize(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .map(|mut entry| {
            entry.command = entry.command.trim().to_owned();
            entry
        })
        .filter(|entry| !entry.command.is_empty())
        .filter(|entry| !entry.command.chars().any(char::is_control))
        .filter(|entry| !is_als_command(&entry.command))
        .filter(|entry| seen.insert(entry.command.clone()))
        .collect()
}

fn is_als_command(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if first == "als" || first.rsplit('/').next() == Some("als") {
        return true;
    }
    first == "command"
        && words
            .next()
            .is_some_and(|word| word == "als" || word.rsplit('/').next() == Some("als"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_newest_first_history() {
        let entries = normalize(vec![
            entry("als add -l2 gs", 5),
            entry("  git status  ", 4),
            entry("git status", 3),
            entry("cargo test", 2),
            entry("", 1),
        ]);
        assert_eq!(entries, [entry("git status", 4), entry("cargo test", 2)]);
    }

    #[test]
    fn parses_timestamps_from_shell_history() {
        let fish = b"1793423529|git status\0";
        let parsed = fish
            .split(|byte| *byte == 0)
            .map(String::from_utf8_lossy)
            .map(|value| parse_timestamped(value.as_ref()))
            .collect::<Vec<_>>();
        assert_eq!(parsed[0], entry("git status", 1_793_423_529));

        let bash = parse_bash("    1  1793423500|git status\n    2  1793423529|cargo test\n");
        assert_eq!(
            bash,
            [
                entry("cargo test", 1_793_423_529),
                entry("git status", 1_793_423_500)
            ]
        );
    }

    #[test]
    fn recognizes_manager_commands() {
        assert!(is_als_command("als h"));
        assert!(is_als_command("command als history"));
        assert!(is_als_command("/usr/local/bin/als add -l"));
        assert!(!is_als_command("echo als h"));
    }

    fn entry(command: &str, timestamp: i64) -> HistoryEntry {
        HistoryEntry {
            command: command.to_owned(),
            timestamp: Some(timestamp),
        }
    }
}
