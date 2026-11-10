//! What the service says about its own start.
use crate::application::ports::{Event, EventLog};
use crate::domain::auth::AuthConfig;

/// Node says nothing when the signing key cannot be read and fails every rotating token silently; this says so once, at start.
pub fn check_signing_key(log: &dyn EventLog, auth: &AuthConfig) {
    if auth.signing_key_unreadable() {
        log.emit(Event::warn("auth.signing_key_unreadable"));
    }
}

/// The line `wf` and the integration suite wait for before they talk to the service.
pub fn announce_listening(log: &dyn EventLog, port: u16, host: &str) {
    log.emit(Event::info("vm-ops-api.listening").with("port", port).with("host", host));
}

/// The CI port is up, after the admin line the harness waits for.
pub fn announce_ci_listening(log: &dyn EventLog, port: u16) {
    log.emit(Event::info("vm-ops-api.ci_listening").with("port", port));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeEventLog;
    use secrecy::SecretString;

    #[test]
    fn given_a_signing_key_that_cannot_be_read_when_start_checks_it_then_one_warning_names_it_and_a_missing_key_says_nothing() {
        let log = FakeEventLog::default();

        check_signing_key(&log, &AuthConfig::new(SecretString::from("seed"), "", false));
        assert_eq!(log.events(), []);
        check_signing_key(&log, &AuthConfig::new(SecretString::from("seed"), "not a key", false));

        assert_eq!(log.events(), [Event::warn("auth.signing_key_unreadable")]);
    }

    #[test]
    fn given_the_ci_port_when_the_service_announces_it_then_the_event_names_the_port() {
        let log = FakeEventLog::default();

        announce_ci_listening(&log, 8081);

        assert_eq!(log.events(), [Event::info("vm-ops-api.ci_listening").with("port", 8081u16)]);
    }

    #[test]
    fn given_a_port_and_a_host_when_the_service_announces_it_listens_then_the_event_names_both_in_that_order() {
        let log = FakeEventLog::default();

        announce_listening(&log, 8080, "main");

        assert_eq!(log.events(), [Event::info("vm-ops-api.listening").with("port", 8080u16).with("host", "main")]);
    }
}
