use anyhow::{Context, Result};
use dialoguer::{Confirm, FuzzySelect, Input, Select, theme::ColorfulTheme};

use crate::model::{Alias, AliasPatch, normalize_desc, normalize_tags};
use crate::store::Store;
use crate::table;

pub fn run(store: &Store) -> Result<()> {
    let theme = ColorfulTheme::default();
    loop {
        let Some(choice) = Select::with_theme(&theme)
            .with_prompt("What would you like to do?")
            .items(["Add", "Del", "Edit", "List", "Quit"])
            .default(0)
            .interact_opt()
            .context("interactive menu failed")?
        else {
            break;
        };

        match choice {
            0 => add(store, &theme)?,
            1 => delete(store, &theme)?,
            2 => edit(store, &theme, None)?,
            3 => print!("{}", table::render(&store.list()?)),
            _ => break,
        }
    }
    Ok(())
}

pub fn edit_named(store: &Store, name: &str) -> Result<()> {
    let theme = ColorfulTheme::default();
    edit(store, &theme, Some(name))
}

fn add(store: &Store, theme: &ColorfulTheme) -> Result<()> {
    let name = Input::<String>::with_theme(theme)
        .with_prompt("Name")
        .interact_text()
        .context("could not read alias name")?;
    let cmd = Input::<String>::with_theme(theme)
        .with_prompt("Command")
        .interact_text()
        .context("could not read alias command")?;
    let desc = Input::<String>::with_theme(theme)
        .with_prompt("Description (optional)")
        .allow_empty(true)
        .interact_text()
        .context("could not read description")?;
    let tags = Input::<String>::with_theme(theme)
        .with_prompt("Tags (optional, comma-separated)")
        .allow_empty(true)
        .interact_text()
        .context("could not read tags")?;

    if !Confirm::with_theme(theme)
        .with_prompt("Save this alias?")
        .default(true)
        .interact()
        .context("could not confirm add")?
    {
        return Ok(());
    }

    let alias = Alias::new(name, cmd, normalize_desc(Some(desc)), parse_tags(&tags)?)?;
    store.add(alias.clone())?;
    println!("Added alias `{}`.", alias.name);
    Ok(())
}

fn delete(store: &Store, theme: &ColorfulTheme) -> Result<()> {
    let Some(alias) = select_alias(store, theme, "Select an alias to delete")? else {
        return Ok(());
    };
    if !Confirm::with_theme(theme)
        .with_prompt(format!("Delete `{}` -> `{}`?", alias.name, alias.cmd))
        .default(false)
        .interact()
        .context("could not confirm delete")?
    {
        return Ok(());
    }

    store.delete(&alias.name)?;
    println!("Deleted alias `{}`.", alias.name);
    Ok(())
}

fn edit(store: &Store, theme: &ColorfulTheme, name: Option<&str>) -> Result<()> {
    let alias = if let Some(name) = name {
        store
            .list()?
            .into_iter()
            .find(|alias| alias.name == name)
            .ok_or_else(|| anyhow::anyhow!("alias `{name}` was not found"))?
    } else {
        let Some(alias) = select_alias(store, theme, "Select an alias to edit")? else {
            return Ok(());
        };
        alias
    };

    let mut draft = alias.clone();
    loop {
        println!("\nCurrent alias:");
        print!("{}", table::render(std::slice::from_ref(&draft)));

        let Some(choice) = Select::with_theme(theme)
            .with_prompt("Choose a field to edit")
            .items([
                "Name",
                "Command",
                "Description",
                "Tags",
                "Save changes",
                "Cancel",
            ])
            .default(0)
            .interact_opt()
            .context("could not select a field")?
        else {
            println!("No changes saved.");
            return Ok(());
        };

        match choice {
            0 => {
                draft.name = Input::<String>::with_theme(theme)
                    .with_prompt("Name")
                    .with_initial_text(draft.name.clone())
                    .interact_text()
                    .context("could not read alias name")?;
            }
            1 => {
                draft.cmd = Input::<String>::with_theme(theme)
                    .with_prompt("Command")
                    .with_initial_text(draft.cmd.clone())
                    .interact_text()
                    .context("could not read alias command")?;
            }
            2 => {
                let desc = Input::<String>::with_theme(theme)
                    .with_prompt("Description (optional)")
                    .with_initial_text(draft.desc.clone().unwrap_or_default())
                    .allow_empty(true)
                    .interact_text()
                    .context("could not read description")?;
                draft.desc = normalize_desc(Some(desc));
            }
            3 => {
                let tags = Input::<String>::with_theme(theme)
                    .with_prompt("Tags (optional, comma-separated)")
                    .with_initial_text(draft.tags.join(", "))
                    .allow_empty(true)
                    .interact_text()
                    .context("could not read tags")?;
                draft.tags = parse_tags(&tags)?;
            }
            4 => {
                if draft == alias {
                    println!("No changes to save.");
                    return Ok(());
                }

                let original_name = alias.name.clone();
                let updated = store.edit(
                    &original_name,
                    AliasPatch {
                        name: (draft.name != alias.name).then(|| draft.name.clone()),
                        cmd: (draft.cmd != alias.cmd).then(|| draft.cmd.clone()),
                        desc: (draft.desc != alias.desc).then(|| draft.desc.clone()),
                        tags: (draft.tags != alias.tags).then(|| draft.tags.clone()),
                    },
                )?;
                if updated.name == original_name {
                    println!("Updated alias `{}`.", updated.name);
                } else {
                    println!("Updated alias `{original_name}` as `{}`.", updated.name);
                }
                return Ok(());
            }
            _ => {
                println!("No changes saved.");
                return Ok(());
            }
        }
    }
}

fn select_alias(store: &Store, theme: &ColorfulTheme, prompt: &str) -> Result<Option<Alias>> {
    let aliases = store.list()?;
    if aliases.is_empty() {
        println!("No aliases found.");
        return Ok(None);
    }
    let items = aliases
        .iter()
        .map(|alias| format!("{}  {}", alias.name, alias.cmd))
        .collect::<Vec<_>>();
    let selected = FuzzySelect::with_theme(theme)
        .with_prompt(prompt)
        .items(&items)
        .interact_opt()
        .context("alias selection failed")?;
    Ok(selected.map(|index| aliases[index].clone()))
}

fn parse_tags(value: &str) -> Result<Vec<String>> {
    normalize_tags(value.split(',').map(str::to_owned).collect()).map_err(Into::into)
}
