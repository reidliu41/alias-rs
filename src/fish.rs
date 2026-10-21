use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::model::Alias;

/// Renders all managed aliases for the current fish process.
pub fn render_sync(aliases: &[Alias]) -> String {
    let mut script = String::new();
    script.push_str(
        "if set -q __als_managed\n    functions --erase $__als_managed 2>/dev/null\nend\n",
    );

    for alias in aliases {
        writeln!(script, "alias {} {}", quote(&alias.name), quote(&alias.cmd))
            .expect("writing to a string cannot fail");
    }

    script.push_str("set -g __als_managed");
    for alias in aliases {
        write!(script, " {}", quote(&alias.name)).expect("writing to a string cannot fail");
    }
    script.push('\n');
    render_completions(&mut script, aliases);
    script
}

/// Renders initial aliases and the wrapper that refreshes the current shell.
pub fn render_init(aliases: &[Alias]) -> String {
    let mut script = render_sync(aliases);
    script.push_str(
        r#"functions --erase als 2>/dev/null
function als --description 'Manage shell aliases'
    if test (count $argv) -gt 0; and test "$argv[1]" = list; and isatty stdin; and isatty stdout; and isatty stderr
        if not contains -- -h $argv[2..-1]; and not contains -- --help $argv[2..-1]
            set -l __als_result (command als __select $argv[2..-1])
            set -l __als_status $status
            if test $__als_status -ne 0
                return $__als_status
            end

            if test (count $__als_result) -ge 2
                set -l __als_action $__als_result[1]
                set -l __als_command $__als_result[2]
                switch $__als_action
                    case execute
                        eval "$__als_command"
                        return $status
                    case insert
                        read --command="$__als_command" --prompt-str='> ' --shell --local __als_line
                        or return 0
                        if test -n "$__als_line"
                            eval "$__als_line"
                            return $status
                        end
                end
            end
            return 0
        end
    end

    command als $argv
    set -l __als_status $status

    if test $__als_status -eq 0
        if test (count $argv) -eq 0
            command als __sync fish | source
        else
            switch $argv[1]
                case add del edit
                    command als __sync fish | source
            end
        end
    end

    return $__als_status
end
"#,
    );
    script
}

fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

fn render_completions(script: &mut String, aliases: &[Alias]) {
    script.push_str(
        r#"complete --erase --command als
functions --erase __als_needs_command __als_needs_alias 2>/dev/null
function __als_needs_command
    set -l tokens (commandline -opc)
    test (count $tokens) -eq 1
end
function __als_needs_alias
    set -l tokens (commandline -opc)
    test (count $tokens) -eq 2
    and contains -- $tokens[2] del edit
end
complete -c als -f
complete -c als -n '__als_needs_command' -a add -d 'Add an alias'
complete -c als -n '__als_needs_command' -a del -d 'Delete an alias'
complete -c als -n '__als_needs_command' -a edit -d 'Edit an alias'
complete -c als -n '__als_needs_command' -a list -d 'List aliases'
complete -c als -s h -l help -d 'Print help'
complete -c als -s V -l version -d 'Print version'
complete -c als -n '__fish_seen_subcommand_from add edit' -s d -l description -x -d 'Set the description'
complete -c als -n '__fish_seen_subcommand_from add edit list' -s t -l tag -x -d 'Set or filter by tag'
complete -c als -n '__fish_seen_subcommand_from del' -s f -l force -d 'Delete without confirmation'
"#,
    );

    for alias in aliases {
        let description = alias.desc.as_deref().unwrap_or(&alias.cmd);
        writeln!(
            script,
            "complete -c als -n '__als_needs_alias' -a {} -d {}",
            quote(&alias.name),
            quote(description)
        )
        .expect("writing to a string cannot fail");
    }

    let tags = aliases
        .iter()
        .flat_map(|alias| alias.tags.iter())
        .collect::<BTreeSet<_>>();
    for tag in tags {
        writeln!(
            script,
            "complete -c als -n '__fish_seen_subcommand_from add edit list' -s t -l tag -x -a {} -d 'Tag'",
            quote(tag)
        )
        .expect("writing to a string cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_fish_values_without_expansion() {
        assert_eq!(quote("echo '$HOME' \\ ok"), "'echo \\'$HOME\\' \\\\ ok'");
    }

    #[test]
    fn init_contains_aliases_and_wrapper() {
        let aliases = vec![
            Alias::new(
                "gs",
                "git status",
                Some("Git status".into()),
                vec!["git".into()],
            )
            .unwrap(),
        ];
        let script = render_init(&aliases);
        assert!(script.contains("alias 'gs' 'git status'"));
        assert!(script.contains("function als"));
        assert!(script.contains("command als __select"));
        assert!(script.contains("command als __sync fish | source"));
        assert!(script.contains("-a add -d 'Add an alias'"));
        assert!(script.contains("-a 'gs' -d 'Git status'"));
        assert!(script.contains("-a 'git' -d 'Tag'"));
    }
}
