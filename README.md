# alias-rs

`alias-rs` is a small, interactive alias manager for Bash, Fish, and Zsh.
It installs the `als` command and stores aliases for all three shells in one
readable TOML file.

## Install

Install the latest release from [crates.io](https://crates.io/crates/alias-rs):

```console
cargo install alias-rs
```

Enable `als` in each shell you use.

### Fish

Create `~/.config/fish/conf.d/als.fish` containing:

```fish
als init fish | source
```

### Bash

Add this line to `~/.bashrc`:

```bash
source <(als init bash)
```

### Zsh

Add this line to `~/.zshrc`:

```zsh
source <(als init zsh)
```

Restart the shell, or run its initialization line once in the current shell.
Initialization loads the managed aliases and installs completions for commands,
options, alias names, and tags.

## Use

Run `als` without arguments to open the interactive menu:

```console
als
```

The management commands also work directly:

```console
als add gs "git status" -d "Show Git status" -t git
als edit gs "git status --short"
als list
als list --tag git
als del gs
als del gs --force
```

Successful additions show exactly what was saved:

```console
$ als add gs "git status"
Added alias: gs  -->  git status
```

Create an alias from the current shell's live history with `als history` (or
the shorter `als h`). It opens a numbered, fuzzy-searchable list; select a
command, enter an alias name, and it is saved immediately. History times are
shown in a compact relative form such as `42s ago`, `5h ago`, or `2d ago`;
entries whose shell history has no timestamp display `-`.

For the quickest path, `-l` uses the most recent usable command and `-lN`
uses item N from the same numbered history list:

```console
als add -l gs
als add -l2 test
als add --last=12 build
```

Omit the name to be prompted for it:

```console
als add -l
```

History is read fresh from Fish, Bash, or Zsh each time. `als` does not import
or maintain a second history database. Blank entries, duplicates, and `als`
management commands are excluded.

In an initialized interactive shell, `als list` opens a searchable selector:

- Type to fuzzy-search creation times, names, commands, descriptions, and tags.
- Use Up/Down to move and Enter to run the selected command immediately.
- Use Right to put the command in an editable input line; press Enter to run it.
- Use Left, Escape, or Ctrl-C to exit without running anything.

When output is redirected or piped, `als list` keeps its plain table output for
scripts.

Run `als edit NAME` without additional options to open the field editor. It
shows the complete alias, lets you change only Name, Command, Description, or
Tags, and saves all selected changes together.

Descriptions and tags are optional. Repeat `--tag` to assign more than one tag:

```console
als add gp "git push" --description "Push commits" --tag git --tag remote
```

Passing an empty description or tag list through `edit` clears it:

```console
als edit gs --description ""
als edit gs --tag ""
```

## Storage

The catalog is stored at `$XDG_CONFIG_HOME/als/aliases.toml`, or
`~/.config/als/aliases.toml` when `XDG_CONFIG_HOME` is not set:

```toml
version = 1

[aliases]
gs = { cmd = "git status", created_at = "2026-10-30T09:24:17-07:00", desc = "Show Git status", tags = ["git"] }
ll = { cmd = "ls -lah", created_at = "2026-10-30T09:25:03-07:00" }
```

New aliases record their creation time automatically. Aliases created by older
versions remain fully usable and display `-` instead of an invented timestamp.

Set `ALS_CONFIG_DIR` to use another directory. This is useful for testing or
keeping a separate catalog.

Writes are protected by a file lock and committed atomically. Before every
successful change, the previous valid catalog is copied to `aliases.toml.bak`.
Invalid TOML is reported and never replaced.

## Shell behavior

The initialization line defines shell functions for the aliases and a small
`als` wrapper. After `add`, `edit`, or `del`, the wrapper reloads only the
aliases and completions managed by this tool in the current shell session.
Other open sessions pick up the catalog the next time they are started or
initialized. The same catalog can be shared by Bash, Fish, and Zsh.

## License

MIT

