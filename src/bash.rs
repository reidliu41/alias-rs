use std::fmt::Write as _;

use crate::model::Alias;

/// Renders all managed aliases and completions for the current Bash process.
pub fn render_sync(aliases: &[Alias]) -> String {
    let mut script = String::new();
    script.push_str(
        r#"if declare -p __als_managed >/dev/null 2>&1; then
    for __als_name in "${__als_managed[@]}"; do
        unset -f -- "$__als_name" 2>/dev/null || true
    done
fi
"#,
    );

    for alias in aliases {
        writeln!(
            script,
            "unalias -- {} 2>/dev/null || true",
            quote(&alias.name)
        )
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

    script.push_str("__als_managed=(");
    for alias in aliases {
        write!(script, " {}", quote(&alias.name)).expect("writing to a string cannot fail");
    }
    script.push_str(" )\n");
    render_completions(&mut script, aliases);
    script
}

/// Renders initial aliases and the wrapper that refreshes the current Bash process.
pub fn render_init(aliases: &[Alias]) -> String {
    let mut script = render_sync(aliases);
    script.push_str(
        r#"unalias -- als 2>/dev/null || true
unset -f als 2>/dev/null || true
unset -f __als_history_stream 2>/dev/null || true
function __als_history_stream {
    local HISTTIMEFORMAT='%s|'
    builtin history 2>/dev/null
}
function als {
    if (( $# == 0 )); then
        __als_history_stream | command als __menu bash
        local __als_status=${PIPESTATUS[1]}
        if (( __als_status == 0 )); then
            source <(command als __sync bash)
        fi
        return "$__als_status"
    fi

    if [[ ( "$1" == "history" || "$1" == "h" ) && "${2-}" != "-h" && "${2-}" != "--help" ]]; then
        __als_history_stream | command als __history bash
        local __als_status=${PIPESTATUS[1]}
        if (( __als_status == 0 )); then
            source <(command als __sync bash)
        fi
        return "$__als_status"
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
            __als_history_stream | command als __add-history bash "${@:2}"
            local __als_status=${PIPESTATUS[1]}
            if (( __als_status == 0 )); then
                source <(command als __sync bash)
            fi
            return "$__als_status"
        fi
    fi

    if [[ "${1-}" == "list" && -t 0 && -t 1 && -t 2 && "${2-}" != "-h" && "${2-}" != "--help" ]]; then
        local __als_result
        __als_result="$(command als __select "${@:2}")"
        local __als_status=$?
        if (( __als_status != 0 )); then
            return "$__als_status"
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
                    local __als_line
                    IFS= read -e -r -i "$__als_command" -p '> ' __als_line || return 0
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
            add|del|edit) source <(command als __sync bash) ;;
        esac
    fi

    return "$__als_status"
}
"#,
    );
    script
}

fn render_completions(script: &mut String, aliases: &[Alias]) {
    script.push_str(
        r#"unset -f __als_complete 2>/dev/null || true
function __als_complete {
    local __als_current="${COMP_WORDS[COMP_CWORD]-}"
    local __als_previous=""
    local __als_command="${COMP_WORDS[1]-}"
    local __als_candidate
    local -a __als_candidates=()
    local -a __als_aliases=("#,
    );
    for alias in aliases {
        write!(script, " {}", quote(&alias.name)).expect("writing to a string cannot fail");
    }
    script.push_str(" )\n    local -a __als_tags=(");
    for tag in unique_tags(aliases) {
        write!(script, " {}", quote(tag)).expect("writing to a string cannot fail");
    }
    script.push_str(
        r#" )

    if (( COMP_CWORD > 0 )); then
        __als_previous="${COMP_WORDS[COMP_CWORD-1]}"
    fi

    if (( COMP_CWORD == 1 )); then
        __als_candidates=(add history h del edit list -h --help -V --version)
    elif [[ "$__als_previous" == "-t" || "$__als_previous" == "--tag" ]]; then
        __als_candidates=("${__als_tags[@]}")
    elif (( COMP_CWORD == 2 )) && [[ "$__als_command" == "del" || "$__als_command" == "edit" ]]; then
        __als_candidates=("${__als_aliases[@]}")
    else
        case "$__als_command" in
            add) __als_candidates=(-l --last -d --description -t --tag -h --help) ;;
            edit) __als_candidates=(-d --description -t --tag -h --help) ;;
            del) __als_candidates=(-f --force -h --help) ;;
            list) __als_candidates=(-t --tag -h --help) ;;
        esac
    fi

    COMPREPLY=()
    for __als_candidate in "${__als_candidates[@]}"; do
        if [[ "$__als_candidate" == "$__als_current"* ]]; then
            COMPREPLY+=("$__als_candidate")
        fi
    done
}
complete -r als 2>/dev/null || true
complete -o bashdefault -o default -F __als_complete als
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
        assert!(script.contains("command als __menu bash"));
        assert!(script.contains("command als __history bash"));
        assert!(script.contains("command als __add-history bash"));
        assert!(script.contains("local HISTTIMEFORMAT='%s|'"));
        assert!(script.contains("builtin history 2>/dev/null"));
        assert!(script.contains("command als __sync bash"));
        assert!(script.contains("complete -o bashdefault"));
        assert!(script.contains("local -a __als_aliases=( 'gs' )"));
        assert!(script.contains("local -a __als_tags=( 'git' )"));
    }

    #[test]
    fn quotes_single_quotes_for_bash() {
        assert_eq!(quote("echo 'hello'"), "'echo '\\''hello'\\'''");
    }
}
