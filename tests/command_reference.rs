use std::process::Command;

fn help(command: Option<&str>, columns: &str, force_color: bool) -> String {
    let mut process = Command::new(env!("CARGO_BIN_EXE_everledger"));
    if let Some(command) = command {
        process.arg(command);
    }
    if force_color {
        process.env("CLICOLOR_FORCE", "1");
    } else {
        process.env_remove("CLICOLOR_FORCE");
    }
    let output = process
        .env_remove("EVERLEDGER_FOREVERDB_DAILY_REQUESTS")
        .arg("--help")
        .env("COLUMNS", columns)
        .env("TERM", "xterm-256color")
        .env_remove("NO_COLOR")
        .output()
        .expect("run everledger --help");
    assert!(output.status.success(), "help failed for {command:?}");
    assert!(
        output.stderr.is_empty(),
        "help wrote stderr for {command:?}"
    );
    String::from_utf8(output.stdout).expect("UTF-8 help")
}

#[test]
fn readme_command_reference_matches_help() {
    let readme = include_str!("../README.md");
    let parser = everledger::cli::parser();
    let commands = std::iter::once(None).chain(
        parser
            .get_subcommands()
            .map(|command| Some(command.get_name())),
    );
    assert_eq!(
        readme.matches("```text\n").count(),
        parser.get_subcommands().count() + 1
    );
    for command in commands {
        let section = match command {
            None => readme,
            Some(name) => {
                readme
                    .split_once(&format!("\n## {name}\n"))
                    .unwrap_or_else(|| panic!("missing README heading for {name}"))
                    .1
            }
        };
        let section = section.split("\n## ").next().unwrap();
        let documented = section
            .split_once("```text\n")
            .expect("README text block")
            .1
            .split_once("```")
            .expect("closed README text block")
            .0;
        let actual = help(command, "80", false);
        assert!(!actual.is_empty());
        assert_eq!(
            documented, actual,
            "README help for {command:?}; regenerate with python3 scripts/update_command_reference.py target/debug/everledger"
        );
        for (columns, force_color) in [("20", true), ("240", false)] {
            assert_eq!(
                actual,
                help(command, columns, force_color),
                "help depends on terminal settings for {command:?}"
            );
        }
    }
}

#[test]
fn configuration_help_describes_the_binary_and_optional_connector() {
    let text = help(None, "80", false);
    assert!(text.contains("Configuration: standalone everledger binary."));
    assert!(
        text.contains("Only connector transport needs python3 and the dynamic_credentials helper.")
    );
    assert!(!text.contains("Python 3.11+ and curl"));
    assert!(!text.contains("beside everledger_*.py"));
    assert!(text.contains("Data from ForeverDB.net (https://foreverdb.net)"));
}
