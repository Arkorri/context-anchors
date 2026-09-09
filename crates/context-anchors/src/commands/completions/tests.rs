use clap::ValueEnum;
use clap_complete::Shell;

use super::*;

fn generated(shell: Shell) -> String {
    let mut out = Vec::new();
    write_to(shell, &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn every_supported_shell_generates_a_non_empty_script() {
    for shell in Shell::value_variants() {
        let script = generated(*shell);
        assert!(!script.is_empty(), "{shell} generated nothing");
        assert!(
            script.contains("anchr"),
            "{shell} script does not name the binary"
        );
    }
}

#[test]
fn the_bash_script_offers_every_subcommand() {
    let script = generated(Shell::Bash);
    for subcommand in [
        "check",
        "backrefs",
        "rename",
        "coverage",
        "annotate",
        "init",
        "lsp",
        "completions",
    ] {
        assert!(
            script.contains(subcommand),
            "bash completions omit `{subcommand}`"
        );
    }
}
