use std::ffi::OsString;
use std::io::{self, IsTerminal};
use std::num::NonZeroUsize;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use dialoguer::{Confirm, theme::ColorfulTheme};

use crate::model::{Alias, AliasPatch, normalize_desc, normalize_tags};
use crate::selector::{self, Selection};
use crate::store::Store;
use crate::table;
use crate::{bash, fish, zsh};
use crate::{history, interactive};

#[derive(Debug, Parser)]
#[command(
    name = "als",
    version,
    about = "A small, interactive alias manager for Fish, Bash, and Zsh",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add an alias.
    Add(AddArgs),
    /// Add an alias from shell history.
    #[command(visible_alias = "h")]
    History,
    /// Delete an alias.
    Del(DelArgs),
    /// Edit an alias.
    Edit(EditArgs),
    /// List aliases.
    List(ListArgs),
    /// Emit shell initialization code.
    #[command(hide = true)]
    Init(ShellArgs),
    /// Emit alias definitions without the wrapper.
    #[command(name = "__sync", hide = true)]
    Sync(ShellArgs),
    /// Open the interactive alias selector.
    #[command(name = "__select", hide = true)]
    Select(ListArgs),
    /// Open the main menu with live shell history.
    #[command(name = "__menu", hide = true)]
    Menu(ShellArgs),
    /// Open the live shell history selector.
    #[command(name = "__history", hide = true)]
    ShellHistory(ShellArgs),
    /// Add an alias from a numbered live shell history entry.
    #[command(name = "__add-history", hide = true)]
    AddHistory(AddHistoryArgs),
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Alias name.
    name: Option<String>,
    /// Command the alias runs. Quote commands containing spaces.
    cmd: Option<String>,
    /// Use the Nth most recent usable shell history entry (default: 1).
    #[arg(
        short = 'l',
        long = "last",
        value_name = "N",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "1",
        conflicts_with = "cmd"
    )]
    last: Option<NonZeroUsize>,
    /// Optional description.
    #[arg(short = 'd', long = "description")]
    description: Option<String>,
    /// Optional tag. Repeat to add more than one.
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct DelArgs {
    /// Alias name.
    name: String,
    /// Delete without confirmation.
    #[arg(short = 'f', long = "force")]
    force: bool,
}

#[derive(Debug, Args)]
struct EditArgs {
    /// Alias name.
    name: String,
    /// Replacement command. Omit all changes to edit interactively.
    cmd: Option<String>,
    /// Replacement description. Pass an empty string to clear it.
    #[arg(short = 'd', long = "description")]
    description: Option<String>,
    /// Replacement tag. Repeat for multiple tags; pass an empty string to clear all tags.
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    tags: Vec<String>,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Show aliases containing this tag.
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    tag: Option<String>,
}

#[derive(Debug, Args)]
struct ShellArgs {
    /// Shell name: fish, bash, or zsh.
    shell: String,
}

#[derive(Debug, Args)]
struct AddHistoryArgs {
    /// Shell name: fish, bash, or zsh.
    shell: String,
    #[command(flatten)]
    add: AddArgs,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse_from(normalize_last_args(std::env::args_os()));
    let store = Store::discover()?;

    match cli.command {
        None => {
            if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
                bail!("interactive mode requires a terminal; use `als list` instead");
            }
            interactive::run(&store, None)
        }
        Some(Command::Add(args)) => add(&store, args),
        Some(Command::History) => bail!(live_history_hint()),
        Some(Command::Del(args)) => delete(&store, args),
        Some(Command::Edit(args)) => edit(&store, args),
        Some(Command::List(args)) => list(&store, args),
        Some(Command::Init(args)) => emit_shell(&store, &args.shell, true),
        Some(Command::Sync(args)) => emit_shell(&store, &args.shell, false),
        Some(Command::Select(args)) => select(&store, args),
        Some(Command::Menu(args)) => menu_with_history(&store, &args.shell),
        Some(Command::ShellHistory(args)) => history_selector(&store, &args.shell),
        Some(Command::AddHistory(args)) => add_from_history(&store, args),
    }
}

fn normalize_last_args(arguments: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut after_separator = false;
    arguments
        .into_iter()
        .map(|argument| {
            if argument == "--" {
                after_separator = true;
                return argument;
            }
            if after_separator {
                return argument;
            }

            let Some(value) = argument.to_str() else {
                return argument;
            };
            let Some(index) = value.strip_prefix("-l") else {
                return argument;
            };
            if !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()) {
                OsString::from(format!("--last={index}"))
            } else {
                argument
            }
        })
        .collect()
}

fn add(store: &Store, args: AddArgs) -> Result<()> {
    if args.last.is_some() {
        bail!(live_history_hint());
    }
    let name = args
        .name
        .ok_or_else(|| anyhow::anyhow!("missing alias name"))?;
    let cmd = args.cmd.ok_or_else(|| {
        anyhow::anyhow!("missing command; provide one or use `als add -l [NAME]`")
    })?;
    let alias = Alias::new(
        name,
        cmd,
        normalize_desc(args.description),
        normalize_tags(args.tags)?,
    )?;
    store.add(alias.clone())?;
    print!("{}", table::added(&alias));
    Ok(())
}

fn menu_with_history(store: &Store, shell: &str) -> Result<()> {
    if !io::stderr().is_terminal() {
        bail!("interactive mode requires a terminal");
    }
    let entries = history::read_from_stdin(shell)?;
    interactive::run(store, Some(&entries))
}

fn history_selector(store: &Store, shell: &str) -> Result<()> {
    if !io::stderr().is_terminal() {
        bail!("interactive history selection requires a terminal");
    }
    let entries = history::read_from_stdin(shell)?;
    history::select_and_add(store, &entries)?;
    Ok(())
}

fn add_from_history(store: &Store, args: AddHistoryArgs) -> Result<()> {
    let index = args
        .add
        .last
        .ok_or_else(|| anyhow::anyhow!("missing `--last` history index"))?;
    if args.add.cmd.is_some() {
        bail!("a command cannot be combined with `--last`");
    }
    let entries = history::read_from_stdin(&args.shell)?;
    history::add_last(
        store,
        &entries,
        index,
        args.add.name,
        normalize_desc(args.add.description),
        normalize_tags(args.add.tags)?,
    )?;
    Ok(())
}

fn live_history_hint() -> &'static str {
    "live shell history is unavailable; initialize als in your shell and run this command through the shell wrapper"
}

fn delete(store: &Store, args: DelArgs) -> Result<()> {
    if !args.force {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            bail!("deleting non-interactively requires `--force`");
        }

        let alias = store
            .list()?
            .into_iter()
            .find(|alias| alias.name == args.name)
            .ok_or_else(|| anyhow::anyhow!("alias `{}` was not found", args.name))?;
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Delete `{}` -> `{}`?", alias.name, alias.cmd))
            .default(false)
            .interact()
            .context("could not confirm delete")?;
        if !confirmed {
            return Ok(());
        }
    }

    store.delete(&args.name)?;
    println!("Deleted alias `{}`.", args.name);
    Ok(())
}

fn edit(store: &Store, args: EditArgs) -> Result<()> {
    let has_tags = !args.tags.is_empty();
    let patch = AliasPatch {
        name: None,
        cmd: args.cmd,
        desc: args.description.map(|desc| normalize_desc(Some(desc))),
        tags: has_tags
            .then_some(args.tags)
            .map(normalize_tags)
            .transpose()?,
    };

    if patch.is_empty() {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            bail!("interactive editing requires a terminal; provide a command or option");
        }
        return interactive::edit_named(store, &args.name);
    }

    store.edit(&args.name, patch)?;
    println!("Updated alias `{}`.", args.name);
    Ok(())
}

fn list(store: &Store, args: ListArgs) -> Result<()> {
    let aliases = filtered_aliases(store, args.tag.as_deref())?;
    print!("{}", table::render(&aliases));
    Ok(())
}

fn select(store: &Store, args: ListArgs) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!("interactive selection requires a terminal; use `als list` for plain output");
    }

    let aliases = filtered_aliases(store, args.tag.as_deref())?;
    match selector::run(&aliases)? {
        Selection::Execute(command) => println!("execute\n{command}"),
        Selection::Insert(command) => println!("insert\n{command}"),
        Selection::Cancel => {}
    }
    Ok(())
}

fn filtered_aliases(store: &Store, tag: Option<&str>) -> Result<Vec<Alias>> {
    let mut aliases = store.list()?;
    if let Some(tag) = tag {
        aliases.retain(|alias| alias.tags.iter().any(|candidate| candidate == tag));
    }
    Ok(aliases)
}

fn emit_shell(store: &Store, shell: &str, include_wrapper: bool) -> Result<()> {
    let aliases = store.list()?;
    let script = match (shell, include_wrapper) {
        ("fish", true) => fish::render_init(&aliases),
        ("fish", false) => fish::render_sync(&aliases),
        ("bash", true) => bash::render_init(&aliases),
        ("bash", false) => bash::render_sync(&aliases),
        ("zsh", true) => zsh::render_init(&aliases),
        ("zsh", false) => zsh::render_sync(&aliases),
        _ => bail!("unsupported shell `{shell}`; supported shells: fish, bash, zsh"),
    };
    print!("{script}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parses_public_commands_and_options() {
        let cli = Cli::try_parse_from([
            "als",
            "add",
            "gs",
            "git status",
            "-d",
            "Git status",
            "-t",
            "git",
        ])
        .unwrap();
        let Some(Command::Add(args)) = cli.command else {
            panic!("expected add command");
        };
        assert_eq!(args.name.as_deref(), Some("gs"));
        assert_eq!(args.cmd.as_deref(), Some("git status"));
        assert_eq!(args.last, None);
        assert_eq!(args.tags, ["git"]);
    }

    #[test]
    fn parses_history_commands_and_last_indices() {
        assert!(matches!(
            Cli::try_parse_from(["als", "history"]).unwrap().command,
            Some(Command::History)
        ));
        assert!(matches!(
            Cli::try_parse_from(["als", "h"]).unwrap().command,
            Some(Command::History)
        ));

        for (arguments, expected_index, expected_name) in [
            (vec!["als", "add", "-l", "ll"], 1, Some("ll")),
            (vec!["als", "add", "-l2", "gs"], 2, Some("gs")),
            (vec!["als", "add", "--last=12"], 12, None),
        ] {
            let cli = Cli::try_parse_from(normalize_last_args(
                arguments.into_iter().map(OsString::from),
            ))
            .unwrap();
            let Some(Command::Add(args)) = cli.command else {
                panic!("expected add command");
            };
            assert_eq!(args.last.map(NonZeroUsize::get), Some(expected_index));
            assert_eq!(args.name.as_deref(), expected_name);
            assert_eq!(args.cmd, None);
        }

        assert!(
            Cli::try_parse_from(normalize_last_args(
                ["als", "add", "-l0", "bad"].map(OsString::from)
            ))
            .is_err()
        );
    }
}
