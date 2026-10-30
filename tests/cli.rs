use std::env;
use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn als(config_dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_als"));
    command.env("ALS_CONFIG_DIR", config_dir);
    command
}

#[test]
fn command_line_crud_flow() {
    let directory = tempdir().unwrap();

    let output = als(directory.path())
        .args(["add", "gs", "git status", "-d", "Git status", "-t", "git"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = als(directory.path())
        .args(["list", "-t", "git"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("gs"));
    assert!(stdout.contains("Git status"));

    let output = als(directory.path())
        .args(["edit", "gs", "git status --short", "-t", "daily"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = als(directory.path())
        .args(["del", "gs", "--force"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog = fs::read_to_string(directory.path().join("aliases.toml")).unwrap();
    assert!(!catalog.contains("gs ="));
}

#[test]
fn new_aliases_include_creation_times_in_storage_and_lists() {
    let directory = tempdir().unwrap();
    assert!(
        als(directory.path())
            .args(["add", "ll", "ls -lah"])
            .status()
            .unwrap()
            .success()
    );

    let catalog = fs::read_to_string(directory.path().join("aliases.toml")).unwrap();
    assert!(catalog.contains("created_at = \""));

    let output = als(directory.path()).arg("list").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("CREATED"));
    assert!(stdout.contains("ll"));
    assert!(!stdout.contains("-                 ll"));
}

#[test]
fn shell_initializers_emit_aliases_wrappers_and_completions() {
    let directory = tempdir().unwrap();
    assert!(
        als(directory.path())
            .args(["add", "say", "echo 'hello'", "-t", "demo"])
            .status()
            .unwrap()
            .success()
    );

    for (shell, alias_marker, completion_marker) in [
        ("fish", "alias 'say' 'echo \\'hello\\''", "complete -c als"),
        ("bash", "function say {", "complete -o bashdefault"),
        ("zsh", "function say {", "compdef __als_complete als"),
    ] {
        let output = als(directory.path())
            .args(["init", shell])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(alias_marker), "missing alias for {shell}");
        assert!(
            stdout.contains("function als"),
            "missing wrapper for {shell}"
        );
        assert!(
            stdout.contains(completion_marker),
            "missing completion for {shell}"
        );
        assert!(stdout.contains("'say'"));
        assert!(stdout.contains("'demo'"));
    }
}

#[test]
fn bash_aliases_forward_arguments_and_refresh_after_changes() {
    let directory = tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_als");
    let binary_directory = std::path::Path::new(binary).parent().unwrap();
    let path = format!(
        "{}:{}",
        binary_directory.display(),
        env::var("PATH").unwrap_or_default()
    );

    assert!(
        als(directory.path())
            .args([
                "add",
                "say",
                "printf 'hello %s\\n'",
                "-d",
                "Say hello",
                "-t",
                "demo",
            ])
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            r#"source <(als init bash)
say world
COMP_WORDS=(als del s)
COMP_CWORD=2
__als_complete
printf 'completion=%s\n' "${COMPREPLY[@]}"
als add bye "printf 'bye %s\\n'" >/dev/null
bye moon"#,
        ])
        .env("ALS_CONFIG_DIR", directory.path())
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("hello world"));
    assert!(stdout.contains("completion=say"));
    assert!(stdout.contains("bye moon"));
}
