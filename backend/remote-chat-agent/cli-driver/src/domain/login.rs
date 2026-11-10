//! How a provider's CLI is logged in and where it keeps what that writes. The CLI writes its tokens itself, into the directory
//! `provider_env` names for it; the service only starts the login command, reads the prompt it prints, and looks for the files.
use super::driver::{Driver, DRIVERS};

/// How a person completes a login: a code the CLI shows and polls for (`DeviceCode`), or a code the person pastes back (`Paste`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    DeviceCode,
    Paste,
}

impl LoginMethod {
    pub fn parse(name: &str) -> Option<LoginMethod> {
        match name {
            "device_code" => Some(LoginMethod::DeviceCode),
            "paste" => Some(LoginMethod::Paste),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LoginMethod::DeviceCode => "device_code",
            LoginMethod::Paste => "paste",
        }
    }
}

/// The login command's arguments, or none when the CLI has no such flow. Written from the CLIs' documentation, not run against them.
pub fn login_args(driver: Driver, method: LoginMethod) -> Option<Vec<String>> {
    let words: &[&str] = match (driver, method) {
        (Driver::ClaudeAgent, LoginMethod::Paste) => &["auth", "login"],
        (Driver::Codex, LoginMethod::DeviceCode) => &["login", "--device-auth"],
        (Driver::Opencode, LoginMethod::Paste) => &["auth", "login"],
        (Driver::ClaudeAgent, LoginMethod::DeviceCode) | (Driver::Codex, LoginMethod::Paste) | (Driver::Opencode, LoginMethod::DeviceCode) => return None,
    };
    Some(words.iter().map(|w| w.to_string()).collect())
}

/// The methods `driver`'s CLI offers, the first being the default.
pub fn login_methods(driver: Driver) -> Vec<LoginMethod> {
    [LoginMethod::DeviceCode, LoginMethod::Paste].into_iter().filter(|m| login_args(driver, *m).is_some()).collect()
}

/// The files, relative to the provider's directory (`<root>/<program>`), whose presence says the CLI holds a login.
pub fn credential_files(driver: Driver) -> &'static [&'static str] {
    match driver {
        Driver::ClaudeAgent => &[".credentials.json"],
        Driver::Codex => &["auth.json"],
        Driver::Opencode => &["data/opencode/auth.json"],
    }
}

/// The driver whose CLI a command starts: the program itself, or the one `env VAR=value ... <program>` runs (how opencode is started).
pub fn driver_of_command(program: &str, args: &[String]) -> Option<Driver> {
    let named = if program == "env" { args.iter().find(|a| !a.contains('='))?.as_str() } else { program };
    DRIVERS.into_iter().find(|d| d.program() == named)
}

/// What a login command's output line tells of the prompt: the address to open and the code to enter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prompt {
    pub url: Option<String>,
    pub code: Option<String>,
}

fn is_device_code(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
    let parts: Vec<&str> = word.split('-').collect();
    parts.len() == 2 && parts.iter().all(|p| (4..=6).contains(&p.len()) && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// The address and the code in one output line of a login command, each when the line has one.
pub fn read_prompt(line: &str) -> Prompt {
    let mut prompt = Prompt::default();
    for word in line.split_whitespace() {
        if prompt.url.is_none() && (word.starts_with("https://") || word.starts_with("http://")) {
            prompt.url = Some(word.trim_end_matches(['.', ',', ')', ';']).to_string());
        } else if prompt.code.is_none() && is_device_code(word) {
            prompt.code = Some(word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-').to_string());
        }
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_each_driver_when_its_login_methods_are_listed_then_only_the_flows_its_cli_has_are_offered() {
        // Given/When the methods of each driver are listed
        // Then each offers a command, and a method with no command is not offered
        for driver in DRIVERS {
            let methods = login_methods(driver);
            assert!(!methods.is_empty(), "{} has no login", driver.id());
            for method in [LoginMethod::DeviceCode, LoginMethod::Paste] {
                assert_eq!(login_args(driver, method).is_some(), methods.contains(&method), "{} {}", driver.id(), method.name());
            }
        }
        assert_eq!(LoginMethod::parse("paste"), Some(LoginMethod::Paste));
        assert_eq!(LoginMethod::parse("qr"), None);
    }

    #[test]
    fn given_a_command_when_its_driver_is_asked_then_env_wrapped_and_plain_programs_are_found_and_others_are_not() {
        let owned = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Given each driver's program, plain and started through env with its settings first
        for driver in DRIVERS {
            // Then the driver is found either way
            assert_eq!(driver_of_command(driver.program(), &[]), Some(driver));
            assert_eq!(driver_of_command("env", &owned(&["A=1", "B=2", driver.program(), "run"])), Some(driver));
        }
        // And a program that is no provider's, or an env that runs none, belongs to no driver
        assert_eq!(driver_of_command("git", &[]), None);
        assert_eq!(driver_of_command("env", &owned(&["A=1"])), None);
        assert_eq!(driver_of_command("env", &owned(&["A=1", "git"])), None);
    }

    #[test]
    fn given_the_lines_a_login_prints_when_read_then_the_address_and_the_device_code_are_found_and_prose_is_not_taken_for_a_code() {
        // Given the lines of a device flow and of a paste flow
        let address = read_prompt("Open https://auth.example.com/device in your browser.");
        let code = read_prompt("Then enter the code ABCD-1234 to sign in");
        let both = read_prompt("Visit https://auth.example.com/activate (code: WXYZ-9876) now");
        let plain = read_prompt("Waiting for authorization... STATUS-QUO is not a code, nor is ABC-1234567");
        // Then the address has no trailing punctuation, a code is two upper-case groups, and the rest finds nothing
        assert_eq!(address, Prompt { url: Some("https://auth.example.com/device".into()), code: None });
        assert_eq!(code, Prompt { url: None, code: Some("ABCD-1234".into()) });
        assert_eq!(both, Prompt { url: Some("https://auth.example.com/activate".into()), code: Some("WXYZ-9876".into()) });
        assert_eq!(plain, Prompt::default());
    }

    #[test]
    fn given_each_driver_when_its_credential_files_are_named_then_they_lie_below_the_directory_provider_env_gives_it() {
        // Given the directory variables of each driver under a root
        for driver in DRIVERS {
            let env = super::super::provider_env::provider_env(driver, "/r", &[]);
            // Then every credential file is below a directory the driver's CLI is told of (its config dir, CODEX_HOME, or the XDG data home)
            for file in credential_files(driver) {
                let path = format!("/r/{}/{file}", driver.program());
                assert!(env.iter().any(|(_, dir)| path.starts_with(&format!("{dir}/"))), "{} keeps {file} where no variable points", driver.id());
            }
        }
    }
}
