//! Shared project information shown by the CLI, TUI, and GUI.

pub const PROJECT_NAME: &str = "tz_combinator";
pub const PROJECT_DESCRIPTION: &str =
    "Deterministic, bounded combinatorics and data-joining tools for the command line and desktop.";
pub const PROJECT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROJECT_LICENSE: &str = "MIT";
pub const PROJECT_REPOSITORY: &str = "https://github.com/taggedzi/tz_combinator";
pub const PROJECT_ISSUES: &str = "https://github.com/taggedzi/tz_combinator/issues";

/// The concise project information embedded in the CLI help.
pub const ABOUT_HELP: &str = concat!(
    "tz_combinator — ",
    "Deterministic, bounded combinatorics and data-joining tools for the command line and desktop.\n\n",
    "Version: ",
    env!("CARGO_PKG_VERSION"),
    "\nLicense: MIT\nGitHub: https://github.com/taggedzi/tz_combinator\n",
    "Bug reports: https://github.com/taggedzi/tz_combinator/issues\n\n",
    "For troubleshooting, include the version, operating system and architecture,\n",
    "the exact command or UI action, the error code and message, and expected vs.\n",
    "actual behavior. Remove secrets and sensitive input data before reporting."
);

/// Full project information for the CLI about command and UI dialogs.
pub fn about_text() -> String {
    format!(
        "{}\n\n{}\n\nVersion: {}\nLicense: {}\nGitHub: {}\nBug reports: {}\n\n\
         For troubleshooting, include the version, operating system and architecture,\n\
         the exact command or UI action, the error code and message, and expected vs.\n\
         actual behavior. Remove secrets and sensitive input data before reporting.\n\n\
         Runtime: {} {}",
        PROJECT_NAME,
        PROJECT_DESCRIPTION,
        PROJECT_VERSION,
        PROJECT_LICENSE,
        PROJECT_REPOSITORY,
        PROJECT_ISSUES,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_information_is_complete_and_consistent() {
        let text = about_text();
        for value in [
            PROJECT_NAME,
            PROJECT_DESCRIPTION,
            PROJECT_VERSION,
            PROJECT_LICENSE,
            PROJECT_REPOSITORY,
            PROJECT_ISSUES,
        ] {
            assert!(text.contains(value), "missing {value:?} from about text");
        }
        assert!(ABOUT_HELP.contains(PROJECT_VERSION));
        assert!(ABOUT_HELP.contains(PROJECT_ISSUES));
    }
}
