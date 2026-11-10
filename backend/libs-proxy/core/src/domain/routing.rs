//! Only `GET /index/<sparse path>`, `GET /index/config.json`, `GET /dl/<crate>/<version>.crate` and `GET /health` are served.
//! The bucket keys follow what `wf lib release` writes: `libs/index/<sparse path>` and `libs/crates/<name>/<version>.crate`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Health,
    Config,
    Index { key: String },
    Crate { key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    NotFound,
    BadRequest,
    MethodNotAllowed,
}

impl Rejection {
    pub fn status(self) -> u16 {
        match self {
            Rejection::NotFound => 404,
            Rejection::BadRequest => 400,
            Rejection::MethodNotAllowed => 405,
        }
    }
}

/// `1/a`, `2/ab`, `3/a/abc`, `se/rv/service-kit`: where a crate's file lives under the index root (Cargo's registry layout).
pub fn sparse_path(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        0 => String::new(),
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => format!("3/{}/{lower}", &lower[..1]),
        _ => format!("{}/{}/{lower}", &lower[..2], &lower[2..4]),
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn valid_version(version: &str) -> bool {
    version.len() <= 64
        && version.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
        && version.split('.').all(|part| !part.is_empty())
}

pub fn route(method: &str, path: &str) -> Result<Route, Rejection> {
    if method != "GET" {
        return Err(Rejection::MethodNotAllowed);
    }
    let Some(rest) = path.strip_prefix('/') else {
        return Err(Rejection::BadRequest);
    };
    // A real name, version or sparse path never needs an escape or a backslash, so an encoded `/` or `.` is an attack, not a lookup.
    if rest.bytes().any(|b| b == b'%' || b == b'\\' || b == 0 || !b.is_ascii_graphic()) {
        return Err(Rejection::BadRequest);
    }
    if rest.is_empty() {
        return Err(Rejection::NotFound);
    }
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err(Rejection::BadRequest);
    }
    match segments.as_slice() {
        ["health"] => Ok(Route::Health),
        ["index", "config.json"] => Ok(Route::Config),
        ["index", tail @ ..] if (2..=3).contains(&tail.len()) => {
            let name = tail[tail.len() - 1];
            if valid_name(name) && name == name.to_ascii_lowercase() && tail.join("/") == sparse_path(name) {
                Ok(Route::Index { key: format!("libs/index/{}", tail.join("/")) })
            } else {
                Err(Rejection::NotFound)
            }
        }
        ["dl", name, file] => match file.strip_suffix(".crate") {
            Some(version) if valid_name(name) && valid_version(version) => Ok(Route::Crate { key: format!("libs/crates/{name}/{version}.crate") }),
            _ => Err(Rejection::NotFound),
        },
        _ => Err(Rejection::NotFound),
    }
}
