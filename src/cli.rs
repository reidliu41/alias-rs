use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use dialoguer::{Confirm, theme::ColorfulTheme};

use crate::interactive;
use crate::model::{Alias, AliasPatch, normalize_desc, normalize_tags};
use crate::selector::{self, Selection};
use crate::store::Store;
use crate::table;
use crate::{bash, fish, zsh};

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
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Alias name.
    name: String,
    /// Command the alias runs. Quote commands containing spaces.
    cmd: String,
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

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::discover()?;

    match cli.command {
        None => {
            if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
                bail!("interactive mode requires a terminal; use `als list` instead");
            }
            interactive::run(&store)
        }
        Some(Command::Add(args)) => add(&store, args),
        Some(Command::Del(args)) => delete(&store, args),
        Some(Command::Edit(args)) => edit(&store, args),
        Some(Command::List(args)) => list(&store, args),
        Some(Command::Init(args)) => emit_shell(&store, &args.shell, true),
        Some(Command::Sync(args)) => emit_shell(&store, &args.shell, false),
        Some(Command::Select(args)) => select(&store, args),
    }
}

fn add(store: &Store, args: AddArgs) -> Result<()> {
    let alias = Alias::new(
        args.name,
        args.cmd,
        normalize_desc(args.description),
        normalize_tags(args.tags)?,
    )?;
    store.add(alias.clone())?;
    println!("Added alias `{}`.", alias.name);
    Ok(())
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
        assert_eq!(args.name, "gs");
        assert_eq!(args.tags, ["git"]);
    }
}
