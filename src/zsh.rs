use std::fmt::Write as _;

use crate::model::Alias;

/// Renders all managed aliases and completions for the current Zsh process.
pub fn render_sync(aliases: &[Alias]) -> String {
    let mut script = String::new();
    script.push_str(
        r#"if (( ${+__als_managed} )); then
    for __als_name in "${__als_managed[@]}"; do
        unfunction "$__als_name" 2>/dev/null || true
    done
fi
"#,
    );

    for alias in aliases {
        writeln!(script, "unalias {} 2>/dev/null || true", quote(&alias.name))
            .expect("writing to a string cannot fail");
        writeln!(script, "function {} {{", alias.name).expect("writing to a string cannot fail");
        writeln!(
            script,
            "    eval {}",
            quote(&format!("{} \"$@\"", alias.cmd))
        )
        .expect("writing to a string cannot fail");
        script.push_str("}\n");
    }

    script.push_str("typeset -ga __als_managed\n__als_managed=(");
    for alias in aliases {
        write!(script, " {}", quote(&alias.name)).expect("writing to a string cannot fail");
    }
    script.push_str(" )\n");
    render_completions(&mut script, aliases);
    script
}

/// Renders initial aliases and the wrapper that refreshes the current Zsh process.
pub fn render_init(aliases: &[Alias]) -> String {
    let mut script = render_sync(aliases);
    script.push_str(
        r#"unalias als 2>/dev/null || true
unfunction als 2>/dev/null || true
unfunction __als_history_stream 2>/dev/null || true
function __als_history_stream {
    builtin fc -l -n -r -t '%s|' 1 2>/dev/null
}
function als {
    if (( $# == 0 )); then
        __als_history_stream | command als __menu zsh
        local __als_status=${pipestatus[2]}
        if (( __als_status == 0 )); then
            source <(command als __sync zsh)
        fi
        return $__als_status
    fi

    if [[ ( "$1" == "history" || "$1" == "h" ) && "${2-}" != "-h" && "${2-}" != "--help" ]]; then
        __als_history_stream | command als __history zsh
        local __als_status=${pipestatus[2]}
        if (( __als_status == 0 )); then
            source <(command als __sync zsh)
        fi
        return $__als_status
    fi

    if [[ "$1" == "add" ]]; then
        local __als_argument
        local __als_use_history=0
        for __als_argument in "${@:2}"; do
            case "$__als_argument" in
                -l|--last|-l[0-9]*|--last=[0-9]*) __als_use_history=1 ;;
            esac
        done
        if (( __als_use_history )); then
            __als_history_stream | command als __add-history zsh "${@:2}"
            local __als_status=${pipestatus[2]}
            if (( __als_status == 0 )); then
                source <(command als __sync zsh)
            fi
            return $__als_status
        fi
    fi

    if [[ "${1-}" == "list" && -t 0 && -t 1 && -t 2 && "${2-}" != "-h" && "${2-}" != "--help" ]]; then
        local __als_result="$(command als __select "${@:2}")"
        local __als_status=$?
        if (( __als_status != 0 )); then
            return $__als_status
        fi

        if [[ "$__als_result" == *$'\n'* ]]; then
            local __als_action="${__als_result%%$'\n'*}"
            local __als_command="${__als_result#*$'\n'}"
            case "$__als_action" in
                execute)
                    eval "$__als_command"
                    return $?
                    ;;
                insert)
                    local __als_line="$__als_command"
                    vared -p '> ' __als_line || return 0
                    if [[ -n "$__als_line" ]]; then
                        eval "$__als_line"
                        return $?
                    fi
                    ;;
            esac
        fi
        return 0
    fi

    command als "$@"
    local __als_status=$?

    if (( __als_status == 0 )); then
        case "$1" in
            add|del|edit) source <(command als __sync zsh) ;;
        esac
    fi

    return $__als_status
}
"#,
    );
    script
}

fn render_completions(script: &mut String, aliases: &[Alias]) {
    script.push_str(
        r#"unfunction __als_complete 2>/dev/null || true
function __als_complete {
    local __als_command="${words[2]}"
    local __als_previous=""
    local -a __als_candidates
    local -a __als_aliases=("#,
    );
    for alias in aliases {
        let description = alias.desc.as_deref().unwrap_or(&alias.cmd);
        write!(
            script,
            " {}",
            quote(&format!("{}:{description}", alias.name))
        )
        .expect("writing to a string cannot fail");
    }
    script.push_str(" )\n    local -a __als_tags=(");
    for tag in unique_tags(aliases) {
        write!(script, " {}", quote(tag)).expect("writing to a string cannot fail");
    }
    script.push_str(
        r#" )

    if (( CURRENT > 1 )); then
        __als_previous="${words[CURRENT-1]}"
    fi

    if (( CURRENT == 2 )); then
        __als_candidates=(
            'add:Add an alias'
            'history:Add from shell history'
            'h:Add from shell history'
            'del:Delete an alias'
            'edit:Edit an alias'
            'list:List aliases'
            '-h:Print help'
            '--help:Print help'
            '-V:Print version'
            '--version:Print version'
        )
        _describe -t commands 'als command' __als_candidates
        return
    fi

    if [[ "$__als_previous" == '-t' || "$__als_previous" == '--tag' ]]; then
        compadd -- "${__als_tags[@]}"
        return
    fi

    if (( CURRENT == 3 )) && [[ "$__als_command" == 'del' || "$__als_command" == 'edit' ]]; then
        _describe -t aliases 'alias' __als_aliases
        return
    fi

    case "$__als_command" in
        add)
            __als_candidates=(
                '-l:Use the Nth recent history entry'
                '--last:Use the Nth recent history entry'
                '-d:Set the description'
                '--description:Set the description'
                '-t:Set a tag'
                '--tag:Set a tag'
                '-h:Print help'
                '--help:Print help'
            )
            ;;
        edit)
            __als_candidates=(
                '-d:Set the description'
                '--description:Set the description'
                '-t:Set a tag'
                '--tag:Set a tag'
                '-h:Print help'
                '--help:Print help'
            )
            ;;
        del)
            __als_candidates=(
                '-f:Delete without confirmation'
                '--force:Delete without confirmation'
                '-h:Print help'
                '--help:Print help'
            )
            ;;
        list)
            __als_candidates=(
                '-t:Filter by tag'
                '--tag:Filter by tag'
                '-h:Print help'
                '--help:Print help'
            )
            ;;
    esac
    _describe -t options 'option' __als_candidates
}
autoload -Uz compinit
if (( ! ${+functions[compdef]} )); then
    compinit
fi
compdef __als_complete als
"#,
    );
}

fn unique_tags(aliases: &[Alias]) -> Vec<&str> {
    let mut tags = aliases
        .iter()
        .flat_map(|alias| alias.tags.iter().map(String::as_str))
        .collect::<Vec<_>>();
    tags.sort_unstable();
    tags.dedup();
    tags
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_contains_alias_wrapper_and_completion() {
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
        assert!(script.contains("function gs {"));
        assert!(script.contains("eval 'git status \"$@\"'"));
        assert!(script.contains("command als __select"));
        assert!(script.contains("command als __menu zsh"));
        assert!(script.contains("command als __history zsh"));
        assert!(script.contains("command als __add-history zsh"));
        assert!(script.contains("builtin fc -l -n -r -t '%s|' 1"));
        assert!(script.contains("command als __sync zsh"));
        assert!(script.contains("compdef __als_complete als"));
        assert!(script.contains("'gs:Git status'"));
        assert!(script.contains("local -a __als_tags=( 'git' )"));
    }
}
