use clap::CommandFactory;

use super::*;

#[test]
fn the_command_line_definition_is_consistent() {
    Cli::command().debug_assert();
}

use clap::Parser;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap()
}

#[test]
fn each_colour_choice_maps_to_its_anstream_equivalent() {
    assert_eq!(
        anstream::ColorChoice::from(Color::Auto),
        anstream::ColorChoice::Auto
    );
    assert_eq!(
        anstream::ColorChoice::from(Color::Always),
        anstream::ColorChoice::Always
    );
    assert_eq!(
        anstream::ColorChoice::from(Color::Never),
        anstream::ColorChoice::Never
    );
}

#[test]
fn check_defaults_to_human_output_in_auto_colour_and_not_strict() {
    let Command::Check(args) = parse(&["anchr", "check"]).command else {
        panic!("expected check");
    };
    assert_eq!(args.format, Format::Human);
    assert_eq!(args.color, Color::Auto);
    assert!(!args.strict);
    assert!(args.paths.is_empty());
    assert!(args.root.is_none());
}

/// `init` writes AGENTS.md-compatible guidance unless asked otherwise; that default is a product
/// decision, not a clap accident.
#[test]
fn init_defaults_to_the_vendor_neutral_agent_guide() {
    let Command::Init(args) = parse(&["anchr", "init"]).command else {
        panic!("expected init");
    };
    assert_eq!(args.agent, Agent::AgentsMd);
    assert!(!args.force);
    assert!(!args.dry_run);
}

#[test]
fn paths_and_flags_may_be_written_in_either_order() {
    let Command::Check(args) = parse(&["anchr", "check", "a.md", "b.md", "--strict"]).command
    else {
        panic!("expected check");
    };
    assert_eq!(args.paths.len(), 2);
    assert!(args.strict);
}

#[test]
fn completions_requires_a_shell_it_recognises() {
    assert!(Cli::try_parse_from(["anchr", "completions"]).is_err());
    assert!(Cli::try_parse_from(["anchr", "completions", "nonsense"]).is_err());
    assert!(Cli::try_parse_from(["anchr", "completions", "bash"]).is_ok());
}
