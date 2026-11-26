//! Which hosts an explicit proxy may look into. An approved fixture host is intercepted; every other host passes through
//! untouched while a recording runs, and is refused while a replay runs, when nothing may leave the machine.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Intercept,
    Tunnel,
    Refuse,
}

#[derive(Clone, Debug, Default)]
pub struct HostPolicy {
    /// `host` or `host:port`, in any case.
    pub approved: Vec<String>,
    /// Nothing but the approved hosts is answered: what a replay with outbound access blocked needs.
    pub offline: bool,
}

impl HostPolicy {
    fn approves(&self, host: &str, port: u16) -> bool {
        self.approved.iter().any(|entry| match entry.rsplit_once(':').and_then(|(name, p)| p.parse::<u16>().ok().map(|p| (name, p))) {
            Some((name, wanted)) => name.eq_ignore_ascii_case(host) && wanted == port,
            None => entry.eq_ignore_ascii_case(host),
        })
    }

    pub fn route(&self, host: &str, port: u16) -> Route {
        match (self.approves(host, port), self.offline) {
            (true, _) => Route::Intercept,
            (false, false) => Route::Tunnel,
            (false, true) => Route::Refuse,
        }
    }
}

/// What a proxy saw of one connection, by `host:port`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    Intercepted(String),
    Tunnelled(String),
    Refused(String),
    /// The client ended the TLS handshake: it does not trust the test CA (or pins its own).
    RejectedCa(String),
    /// The server's certificate did not validate, so nothing was forwarded.
    UntrustedUpstream(String),
}

/// What a recording does not cover, said so that nobody takes its silence for coverage.
#[derive(serde::Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Gaps {
    /// Hosts that passed through unobserved, with how many connections.
    pub tunnelled: BTreeMap<String, u32>,
    pub refused: BTreeMap<String, u32>,
    /// Approved hosts whose client would not trust the test CA.
    pub rejected_ca: BTreeMap<String, u32>,
    pub untrusted_upstream: BTreeMap<String, u32>,
    /// Approved hosts no connection asked for: the client bypassed the proxy, or never used them.
    pub never_seen: Vec<String>,
}

pub fn gaps(policy: &HostPolicy, seen: &[Seen]) -> Gaps {
    let mut out = Gaps::default();
    let mut reached: Vec<&str> = Vec::new();
    for item in seen {
        let (counts, host) = match item {
            Seen::Intercepted(host) => {
                reached.push(host);
                continue;
            }
            Seen::Tunnelled(host) => (&mut out.tunnelled, host),
            Seen::Refused(host) => (&mut out.refused, host),
            Seen::RejectedCa(host) => {
                reached.push(host);
                (&mut out.rejected_ca, host)
            }
            Seen::UntrustedUpstream(host) => (&mut out.untrusted_upstream, host),
        };
        *counts.entry(host.clone()).or_default() += 1;
    }
    let touched = |entry: &str| reached.iter().any(|host| host.eq_ignore_ascii_case(entry) || host.rsplit_once(':').is_some_and(|(name, _)| name.eq_ignore_ascii_case(entry)));
    out.never_seen = policy.approved.iter().filter(|entry| !touched(entry)).cloned().collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_approved_hosts_when_a_connection_is_routed_then_only_they_are_intercepted_and_a_replay_refuses_the_rest() {
        // Given
        let recording = HostPolicy { approved: vec!["api.fixture.test".into(), "127.0.0.1:8443".into()], offline: false };
        let replaying = HostPolicy { offline: true, ..recording.clone() };

        // When / Then
        assert_eq!(recording.route("API.fixture.test", 443), Route::Intercept);
        assert_eq!((recording.route("127.0.0.1", 8443), recording.route("127.0.0.1", 9000)), (Route::Intercept, Route::Tunnel));
        assert_eq!((recording.route("updates.example.org", 443), replaying.route("updates.example.org", 443)), (Route::Tunnel, Route::Refuse));
    }

    #[test]
    fn given_what_a_proxy_saw_when_the_gaps_are_named_then_bypassed_rejected_and_unobserved_hosts_are_each_said() {
        // Given
        let policy = HostPolicy { approved: vec!["api.fixture.test".into(), "pinned.fixture.test".into(), "unused.fixture.test".into()], offline: false };
        let seen = vec![
            Seen::Intercepted("api.fixture.test:443".into()),
            Seen::RejectedCa("pinned.fixture.test:443".into()),
            Seen::Tunnelled("updates.example.org:443".into()),
            Seen::Tunnelled("updates.example.org:443".into()),
        ];

        // When
        let got = gaps(&policy, &seen);

        // Then
        assert_eq!(got.never_seen, ["unused.fixture.test"]);
        assert_eq!(got.rejected_ca["pinned.fixture.test:443"], 1);
        assert_eq!(got.tunnelled["updates.example.org:443"], 2);
    }
}
