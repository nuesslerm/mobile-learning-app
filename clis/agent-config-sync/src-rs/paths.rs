/// Node `path.posix.normalize` for the relative and absolute paths this tool builds.
pub fn normalize(p: &str) -> String {
    let absolute = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(out.last(), Some(last) if *last != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

pub fn join(parts: &[&str]) -> String {
    normalize(&parts.join("/"))
}

/// Node `path.relative(from, to)` for two paths relative to the same directory.
pub fn relative(from: &str, to: &str) -> String {
    let a = normalize(from);
    let b = normalize(to);
    let a: Vec<&str> = a.split('/').filter(|s| *s != ".").collect();
    let b: Vec<&str> = b.split('/').filter(|s| *s != ".").collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/")
}

pub fn basename(p: &str, suffix: &str) -> String {
    let name = p.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    name.strip_suffix(suffix)
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}
