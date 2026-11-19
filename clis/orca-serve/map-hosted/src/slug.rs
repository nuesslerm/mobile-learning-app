//! The host's reading of a repository's GitHub slug from its git remotes, pure: which remote stands for origin, and the owner and repository a
//! github.com remote URL names. Transcribed from Orca's `parseGitHubRemoteIdentity`, `normalizeGitRemoteUrl` and `deriveOriginStandInRemoteName`;
//! URLs are read the way the WHATWG parser the host uses reads them. Also the commit page link of a github.com, gitlab.com or bitbucket.org remote
//! (`parseHostedRemote` and `buildHostedRemoteCommitUrl`).
use serde_json::Value;
use std::cmp::Ordering;

const PROTOCOLS: &[&str] = &["git", "git+ssh", "http", "https", "ssh"];

/// A URL as far as the host reads one: its lowercase scheme, its host (with a non-default port), its hostname and its path.
#[derive(Debug, PartialEq)]
struct Url {
    scheme: String,
    host: String,
    hostname: String,
    path: String,
}

fn special(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp" | "file")
}

fn default_port(scheme: &str) -> Option<&'static str> {
    match scheme {
        "http" | "ws" => Some("80"),
        "https" | "wss" => Some("443"),
        "ftp" => Some("21"),
        _ => None,
    }
}

fn encode_path(segment: &str) -> String {
    let mut out = String::new();
    for c in segment.chars() {
        if c.is_ascii_control() || matches!(c, ' ' | '"' | '<' | '>' | '`' | '{' | '}') || !c.is_ascii() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The path with its dot segments resolved, as the parser leaves it.
fn resolve_dots(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *segment {
            "." if last => parts.push(""),
            "." => {}
            ".." => {
                parts.pop();
                if last {
                    parts.push("");
                }
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

fn forbidden_host(c: char) -> bool {
    matches!(c, '\0' | '\t' | '\n' | '\r' | ' ' | '#' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|')
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The hostname the parser keeps, or none where it throws: a special host is percent-decoded and lowercased, an opaque one kept as written.
fn parse_host(raw: &str, is_special: bool) -> Option<String> {
    if raw.starts_with('[') {
        return raw.ends_with(']').then(|| raw.to_ascii_lowercase());
    }
    if !is_special {
        return (!raw.chars().any(forbidden_host)).then(|| raw.to_string());
    }
    let domain = percent_decode(raw).to_lowercase();
    (!domain.chars().any(|c| forbidden_host(c) || c.is_ascii_control() || c == '%')).then_some(domain)
}

fn url_path(raw: &str, is_special: bool) -> String {
    if raw.is_empty() {
        return if is_special { "/".to_string() } else { String::new() };
    }
    let raw = if raw.starts_with('/') { raw.to_string() } else { format!("/{raw}") };
    resolve_dots(&encode_path(&raw))
}

/// A `file:` URL: only what follows `//` is its host, a drive letter there is path, and `localhost` is no host.
fn file_url(rest: &str) -> Option<Url> {
    let (authority, tail) = match rest.strip_prefix("//") {
        Some(after) => after.split_at(after.find(['/', '?', '#']).unwrap_or(after.len())),
        None => ("", rest),
    };
    let bytes = authority.as_bytes();
    let drive = bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && matches!(bytes[1], b':' | b'|');
    let (hostname, tail) = if drive || authority.is_empty() {
        (String::new(), if drive { rest.trim_start_matches('/') } else { tail })
    } else {
        let hostname = parse_host(authority, true)?;
        (if hostname == "localhost" { String::new() } else { hostname }, tail)
    };
    let raw_path = &tail[..tail.find(['?', '#']).unwrap_or(tail.len())];
    Some(Url { scheme: "file".into(), host: hostname.clone(), hostname, path: url_path(raw_path, true) })
}

/// An absolute URL with an authority, or none where the host's parser throws or gives no authority.
fn parse_url(input: &str) -> Option<Url> {
    let input: String = input.trim_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let colon = input.find(':')?;
    let scheme = input[..colon].to_ascii_lowercase();
    if !scheme.starts_with(|c: char| c.is_ascii_alphabetic()) || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let is_special = special(&scheme);
    let rest = if is_special { input[colon + 1..].replace('\\', "/") } else { input[colon + 1..].to_string() };
    if scheme == "file" {
        return file_url(&rest);
    }
    let rest = if is_special { rest.trim_start_matches('/') } else { rest.strip_prefix("//")? };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let tail = &rest[end..];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (hostname, port) = match host_port.rfind(':') {
        Some(at) if !host_port.ends_with(']') => (&host_port[..at], Some(&host_port[at + 1..])),
        _ => (host_port, None),
    };
    let port = match port {
        Some(p) if !p.is_empty() => {
            if !p.chars().all(|c| c.is_ascii_digit()) || p.parse::<u32>().ok()? > 65535 {
                return None;
            }
            Some(p.trim_start_matches('0').to_string()).map(|p| if p.is_empty() { "0".to_string() } else { p })
        }
        _ => None,
    };
    if is_special && hostname.is_empty() {
        return None;
    }
    let hostname = parse_host(hostname, is_special)?;
    let port = port.filter(|p| Some(p.as_str()) != default_port(&scheme));
    let host = match &port {
        Some(p) => format!("{hostname}:{p}"),
        None => hostname.clone(),
    };
    let raw_path = &tail[..tail.find(['?', '#']).unwrap_or(tail.len())];
    Some(Url { scheme, host, hostname, path: url_path(raw_path, is_special) })
}

fn strip_git_suffix_ci(name: &str) -> &str {
    let n = name.len();
    if n >= 4 && name.is_char_boundary(n - 4) && name[n - 4..].eq_ignore_ascii_case(".git") {
        &name[..n - 4]
    } else {
        name
    }
}

fn github_host(host: &str) -> String {
    let host = host.trim().to_lowercase();
    if host == "ssh.github.com" {
        "github.com".into()
    } else {
        host
    }
}

/// The scp form `user@host:owner/repo[.git]`, exactly one `/` after the colon.
fn scp_identity(url: &str) -> Option<(String, String, String)> {
    let (user, rest) = url.split_once('@')?;
    if user.is_empty() || user.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
        return None;
    }
    let (host, path) = rest.split_once(':')?;
    let (owner, repo) = path.split_once('/')?;
    if host.is_empty() || owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    let stripped = strip_git_suffix_ci(repo);
    let repo = if stripped.is_empty() { repo } else { stripped };
    Some((github_host(host), owner.to_string(), repo.to_string()))
}

/// `parseGitHubRemoteIdentity`: the host, owner and repository a remote URL names.
fn identity(remote_url: &str) -> Option<(String, String, String)> {
    let trimmed = remote_url.trim();
    if let Some(found) = scp_identity(trimmed) {
        return Some(found);
    }
    let url = parse_url(trimmed)?;
    if !PROTOCOLS.contains(&url.scheme.as_str()) {
        return None;
    }
    let path = url.path.trim_start_matches('/').trim_end_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    let [owner, repo] = parts[..] else { return None };
    let repo = strip_git_suffix_ci(repo);
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let host = if matches!(url.scheme.as_str(), "http" | "https") { url.host } else { url.hostname };
    Some((github_host(&host), owner.to_string(), repo.to_string()))
}

/// The owner and repository of a remote URL that names a github.com repository.
pub fn github_com_slug(remote_url: &str) -> Option<(String, String)> {
    identity(remote_url).filter(|(host, ..)| host == "github.com").map(|(_, owner, repo)| (owner, repo))
}

/// `parseRemoteProjectRefCandidate` with the built-in known host: whether a remote URL names a gitlab.com project (a host of `gitlab.com` on any
/// port, or its SSH-over-443 alias, and a path of at least two segments).
pub fn is_gitlab_com_project(remote_url: &str) -> bool {
    let trimmed = remote_url.trim();
    let has_scheme = trimmed.split_once("://").is_some_and(|(s, _)| s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')));
    let scp = (!has_scheme).then(|| scp_project(trimmed)).flatten();
    let (host, path) = match scp {
        Some(found) => found,
        None => match parse_url(trimmed).filter(|u| PROTOCOLS.contains(&u.scheme.as_str())) {
            Some(u) => (if matches!(u.scheme.as_str(), "http" | "https") { u.host } else { u.hostname }, u.path),
            None => return false,
        },
    };
    let host = host.trim().to_lowercase();
    let hostname = host.rsplit_once(':').filter(|(_, p)| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).map_or(host.as_str(), |(h, _)| h);
    let path = path.trim_start_matches('/').trim_end_matches('/');
    let path = strip_git_suffix_ci(path).trim();
    matches!(hostname, "gitlab.com" | "altssh.gitlab.com") && path.contains('/')
}

/// The scp form `[user@]host:path[.git]` as the GitLab parser matches it.
fn scp_project(url: &str) -> Option<(String, String)> {
    let rest = match url.split_once('@') {
        Some((user, rest)) if !user.is_empty() && !user.contains(['/', ':']) => rest,
        _ => url,
    };
    let (host, path) = rest.split_once(':')?;
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/') || path.is_empty() || path.contains(char::is_whitespace) {
        return None;
    }
    Some((host.to_string(), path.strip_suffix(".git").filter(|p| !p.is_empty()).unwrap_or(path).to_string()))
}

fn remote_path(path: &str) -> &str {
    let path = path.trim_start_matches('/').trim_end_matches('/');
    path.strip_suffix(".git").unwrap_or(path)
}

/// `normalizeGitRemoteUrl` accepts the URL: a network remote, never a local path.
fn network_remote(url: &str) -> bool {
    let trimmed = url.trim();
    let bytes = trimmed.as_bytes();
    if trimmed.is_empty() || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')) {
        return false;
    }
    if !trimmed.contains("://") {
        if let Some((before, path)) = trimmed.split_once(':') {
            if !before.is_empty() && !path.is_empty() && !before.chars().any(char::is_whitespace) {
                return !remote_path(path).is_empty();
            }
        }
    }
    parse_url(trimmed).is_some_and(|u| !u.hostname.trim().is_empty() && !remote_path(&u.path).is_empty())
}

fn primary_rank(name: &str) -> u8 {
    match name {
        "upstream" => 0,
        "origin" => 1,
        _ => 2,
    }
}

fn locale_order(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// `deriveOriginStandInRemoteName`: the remote that plays origin's role when origin is missing or a local path, none when origin plays it itself.
pub fn stand_in(remotes: &[Value]) -> Option<String> {
    let named = |r: &Value| r["name"].as_str().map(str::to_string);
    if !remotes.iter().filter_map(named).any(|n| n != "origin" && n != "upstream") {
        return None;
    }
    let mut network: Vec<String> = remotes.iter().filter(|r| r["url"].as_str().is_some_and(network_remote)).filter_map(named).collect();
    network.sort_by(|a, b| primary_rank(a).cmp(&primary_rank(b)).then_with(|| locale_order(a, b)));
    network.into_iter().find(|n| n != "upstream").filter(|n| n != "origin")
}

/// JavaScript's whitespace and line terminators (what `trim()` and `\s` mean to the host).
fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

/// `decodeURIComponent`, or none where it throws (a malformed escape, or bytes that are not UTF-8).
fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = |b: Option<&u8>| b.and_then(|b| (*b as char).to_digit(16));
            out.push((hex(bytes.get(i + 1))? * 16 + hex(bytes.get(i + 2))?) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `encodeURIComponent`: everything but the unreserved marks, as uppercase escapes of its UTF-8 bytes.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The three public forges, with their SSH-over-443 aliases folded; any other host (www., Enterprise, a mirror) is none.
fn forge(host: &str) -> Option<(&'static str, &'static str)> {
    match host.trim().to_lowercase().as_str() {
        "github.com" | "ssh.github.com" => Some(("github.com", "github")),
        "gitlab.com" | "altssh.gitlab.com" => Some(("gitlab.com", "gitlab")),
        "bitbucket.org" | "ssh.bitbucket.org" => Some(("bitbucket.org", "bitbucket")),
        _ => None,
    }
}

/// `cleanRemotePath`: at least two segments, one `.git` gone, each segment decoded where it decodes.
fn clean_remote_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = strip_git_suffix_ci(path.trim_start_matches('/').trim_end_matches('/')).split('/').filter(|p| !p.is_empty()).collect();
    (parts.len() >= 2).then(|| parts.iter().map(|p| decode_uri_component(p).unwrap_or_else(|| p.to_string())).collect::<Vec<_>>().join("/"))
}

/// The scp form `[user@]host:path` exactly as the host's pattern matches it: the user's part is tried first, then the whole as the host.
fn scp_forge(text: &str) -> Option<(String, String)> {
    let split = |rest: &str| {
        let (host, path) = rest.split_once(':')?;
        let host_ok = !host.is_empty() && !host.chars().any(|c| c == '/' || js_space(c));
        (host_ok && !path.is_empty() && !path.chars().any(js_space)).then(|| (host.to_string(), path.to_string()))
    };
    let with_user = text.split_once('@').filter(|(user, _)| !user.is_empty() && !user.contains(['/', ':'])).and_then(|(_, rest)| split(rest));
    with_user.or_else(|| split(text))
}

/// `parseHostedRemote`: the forge host, its path and its provider, from a shorthand, an scp form or a URL of git, http, https or ssh.
fn hosted_remote(remote_url: &str) -> Option<(&'static str, String, &'static str)> {
    let trimmed = remote_url.trim_matches(js_space);
    let trimmed = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    let letters = trimmed.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters > 0 && trimmed.as_bytes().get(letters) == Some(&b':') {
        let rest = &trimmed[letters + 1..];
        let mut chars = rest.chars();
        let shorthand = chars.next().is_some_and(|c| c != '/') && chars.clone().next().is_some() && !chars.any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
        if shorthand {
            let (host, provider) = match trimmed[..letters].to_ascii_lowercase().as_str() {
                "github" => ("github.com", "github"),
                "gitlab" => ("gitlab.com", "gitlab"),
                "bitbucket" => ("bitbucket.org", "bitbucket"),
                _ => return None,
            };
            return clean_remote_path(rest).map(|path| (host, path, provider));
        }
    }
    let scheme = trimmed.split_once("://").is_some_and(|(s, _)| s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')));
    if !scheme {
        if let Some((host, path)) = scp_forge(trimmed) {
            let (host, provider) = forge(&host)?;
            return clean_remote_path(&path).map(|path| (host, path, provider));
        }
    }
    let url = parse_url(trimmed).filter(|u| matches!(u.scheme.as_str(), "git" | "http" | "https" | "ssh"))?;
    let (host, provider) = forge(&url.hostname)?;
    clean_remote_path(&url.path).map(|path| (host, path, provider))
}

/// `buildHostedRemoteCommitUrl`: the forge's page of a commit, none for a remote no public forge hosts.
pub fn hosted_commit_url(remote_url: &str, sha: &str) -> Option<String> {
    let sha = sha.trim_matches(js_space);
    if sha.is_empty() {
        return None;
    }
    let (host, path, provider) = hosted_remote(remote_url)?;
    let base = format!("https://{host}/{}", path.split('/').map(encode_uri_component).collect::<Vec<_>>().join("/"));
    let sha = encode_uri_component(sha);
    Some(match provider {
        "gitlab" => format!("{base}/-/commit/{sha}"),
        "bitbucket" => format!("{base}/commits/{sha}"),
        _ => format!("{base}/commit/{sha}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn slug(url: &str) -> Option<(String, String)> {
        github_com_slug(url)
    }

    fn pair(owner: &str, repo: &str) -> Option<(String, String)> {
        Some((owner.into(), repo.into()))
    }

    #[test]
    fn given_remote_urls_when_read_as_gitlab_projects_then_only_a_gitlab_com_path_of_two_segments_is_one() {
        for url in ["git@gitlab.com:grp/app.git", "https://gitlab.com/grp/sub/app", "https://tok@GitLab.com:8443/grp/app.git", "ssh://git@altssh.gitlab.com:443/grp/app", "gitlab.com:grp/app"] {
            assert!(is_gitlab_com_project(url), "{url}");
        }
        for url in ["git@gitlab.com:app.git", "https://gitlab.example/grp/app", "https://github.com/acme/app", "file:///srv/gitlab.com/grp/app", "/srv/repo.git", "ftp://gitlab.com/grp/app", ""] {
            assert!(!is_gitlab_com_project(url), "{url}");
        }
    }

    #[test]
    fn given_the_remote_forms_the_host_accepts_when_read_then_owner_and_repo_keep_their_case_and_lose_one_git_suffix() {
        assert_eq!(slug("git@github.com:Octo/Hello.git"), pair("Octo", "Hello"));
        assert_eq!(slug("org-123@github.com:acme/app.GIT"), pair("acme", "app"));
        assert_eq!(slug("  git@GitHub.com:acme/app  "), pair("acme", "app"));
        assert_eq!(slug("git@github.com:acme/.git"), pair("acme", ".git"));
        assert_eq!(slug("git@github.com:acme/a.git.git"), pair("acme", "a.git"));
        assert_eq!(slug("https://github.com/acme/app.git"), pair("acme", "app"));
        assert_eq!(slug("https://user:pw@GITHUB.com/acme/app/"), pair("acme", "app"));
        assert_eq!(slug("https://github.com:443/acme/app"), pair("acme", "app"));
        assert_eq!(slug("HTTPS://github.com/acme/app?x=1#y"), pair("acme", "app"));
        assert_eq!(slug("ssh://git@github.com:22/acme/app.git"), pair("acme", "app"));
        assert_eq!(slug("ssh://git@ssh.github.com:443/acme/app.git"), pair("acme", "app"));
        assert_eq!(slug("git+ssh://git@github.com/acme/app"), pair("acme", "app"));
        assert_eq!(slug("git://github.com/acme/app.git"), pair("acme", "app"));
        assert_eq!(slug("https://github.com/acme/./x/../app"), pair("acme", "app"));
        assert_eq!(slug("https://github.com/acme/my app"), pair("acme", "my%20app"));
    }

    #[test]
    fn given_remotes_that_name_no_github_com_repository_when_read_then_there_is_no_slug() {
        for url in [
            "",
            "/srv/local.git",
            "C:\\repos\\app",
            "git@gitlab.com:grp/proj.git",
            "git@github.com:grp/sub/proj.git",
            "git@github.com:proj.git",
            "https://gitlab.com/acme/app",
            "https://ghe.example.com/acme/app",
            "https://github.com:8443/acme/app",
            "http://github.com:443/acme/app",
            "https://github.com/acme",
            "https://github.com/acme/app/pulls",
            "https://github.com/acme//app",
            "https://github.com/acme/.git",
            "ssh://git@github.com:x/acme/app",
            "ftp://github.com/acme/app",
            "file:///github.com/acme/app",
            "ssh:github.com/acme/app",
            "git@github-work:acme/app.git",
        ] {
            assert_eq!(slug(url), None, "{url}");
        }
    }

    fn remotes(pairs: &[(&str, &str)]) -> Vec<Value> {
        pairs.iter().map(|(n, u)| json!({"name": n, "url": u})).collect()
    }

    #[test]
    fn given_the_remotes_of_a_repository_when_origin_s_stand_in_is_picked_then_it_is_the_first_network_remote_that_is_not_upstream_unless_that_is_origin() {
        assert_eq!(stand_in(&remotes(&[])), None);
        assert_eq!(stand_in(&remotes(&[("origin", "/srv/local.git"), ("upstream", "https://github.com/a/b")])), None);
        assert_eq!(stand_in(&remotes(&[("origin", "/srv/local.git"), ("fork", "https://github.com/me/app.git")])), Some("fork".into()));
        assert_eq!(stand_in(&remotes(&[("origin", "git@gitlab.com:g/p.git"), ("fork", "https://github.com/me/app.git")])), None);
        assert_eq!(stand_in(&remotes(&[("upstream", "https://github.com/a/b"), ("zeta", "git@github.com:z/app.git"), ("Alpha", "/local")])), Some("zeta".into()));
        assert_eq!(stand_in(&remotes(&[("zeta", "https://github.com/z/app"), ("Beta", "https://github.com/b/app"), ("alpha", "C:/x")])), Some("Beta".into()));
        assert_eq!(stand_in(&remotes(&[("b", "host:path"), ("a", "host:/")])), Some("b".into()));
        assert_eq!(stand_in(&remotes(&[("origin", "file:///srv/local.git"), ("fork", "https://github.com/me/app")])), Some("fork".into()));
    }

    #[test]
    fn given_remote_urls_when_the_host_asks_whether_each_is_a_network_remote_then_file_urls_without_a_host_and_invalid_urls_are_not() {
        for url in [
            "file:///srv/x.git",
            "file://localhost/srv/x.git",
            "FILE://LocalHost/srv/x.git",
            "file://C:/repos/app.git",
            "file://user@server/srv/x.git",
            "file://server:22/srv/x.git",
            "https://git hub.com/a/b",
            "https://github.com:443:/a/b",
            "https://git%20hub.com/a/b",
            "ssh://git hub/a/b",
            "https://github.com/",
            "file:////srv/x",
            "ssh:///x/y",
            "https://%zz.com/a",
        ] {
            assert!(!network_remote(url), "{url}");
        }
        for url in ["file://server/share/x.git", "file:/srv/x.git", "https://github%2Ecom/a/b", "https://[::1]:8443/a/b", "ssh://git@host/a/b", "https://a@b@github.com/x/y", "host:path"] {
            assert!(network_remote(url), "{url}");
        }
    }
    #[test]
    fn given_remote_urls_when_a_commit_page_is_built_then_only_the_three_public_forges_have_one_in_their_own_form() {
        let sha = "AbCdEf0123456789abcdef0123456789abcdef01";
        let page = |url: &str| hosted_commit_url(url, sha);
        // Given the scp, ssh, https, shorthand and git+ forms of the three forges, When a commit page is built, Then it is the forge's own page
        assert_eq!(page("git@github.com:Octo/Hello.git").as_deref(), Some("https://github.com/Octo/Hello/commit/AbCdEf0123456789abcdef0123456789abcdef01"));
        assert_eq!(page("ssh://git@ssh.github.com:443/a/b.git").as_deref(), Some(format!("https://github.com/a/b/commit/{sha}").as_str()));
        assert_eq!(page(" https://user:tok@github.com/o/r/ \n").as_deref(), Some(format!("https://github.com/o/r/commit/{sha}").as_str()));
        assert_eq!(page("https://gitlab.com/grp/sub/proj.GIT").as_deref(), Some(format!("https://gitlab.com/grp/sub/proj/-/commit/{sha}").as_str()));
        assert_eq!(page("git@altssh.gitlab.com:grp/proj.git").as_deref(), Some(format!("https://gitlab.com/grp/proj/-/commit/{sha}").as_str()));
        assert_eq!(page("git@bitbucket.org:team/repo.git").as_deref(), Some(format!("https://bitbucket.org/team/repo/commits/{sha}").as_str()));
        assert_eq!(page("gitlab:grp/proj").as_deref(), Some(format!("https://gitlab.com/grp/proj/-/commit/{sha}").as_str()));
        assert_eq!(page("git+ssh://git@github.com/o/r.git").as_deref(), Some(format!("https://github.com/o/r/commit/{sha}").as_str()));
        // And a path segment is decoded then encoded again, so a space round-trips and an encoded slash becomes a separator
        assert_eq!(page("https://github.com/o/my%20repo").as_deref(), Some(format!("https://github.com/o/my%20repo/commit/{sha}").as_str()));
        assert_eq!(page("https://gitlab.com/grp/a%2Fb").as_deref(), Some(format!("https://gitlab.com/grp/a/b/-/commit/{sha}").as_str()));
        assert_eq!(page("https://github.com/o/bad%zz").as_deref(), Some(format!("https://github.com/o/bad%25zz/commit/{sha}").as_str()));
        // Given remotes no public forge hosts, or with fewer than two path segments, Then there is no page
        for none in ["https://www.github.com/a/b", "https://ghe.example.com/a/b", "/srv/local.git", "file:///srv/x", "https://github.com/onlyowner", "localhost:o/r", "https:x/y", "GIT+ssh://git@github.com/o/r", "ftp://github.com/o/r", "unknown:o/r", ""] {
            assert_eq!(page(none), None, "{none}");
        }
        // And an empty sha has no page
        assert_eq!(hosted_commit_url("git@github.com:o/r", "  "), None);
    }

}
