/// The process a record comes from, set once: the service name it was given and what is inferred around it.
#[derive(Clone, Debug)]
pub struct Source {
    pub service: String,
    /// The calling crate's version, compiled in (`source!`).
    pub version: &'static str,
    /// The commit the image was built from (`GIT_COMMIT`), when known.
    pub commit: Option<String>,
    pub host: Option<String>,
}

impl Source {
    pub fn new(service: impl Into<String>, version: &'static str) -> Self {
        let commit = std::env::var("GIT_COMMIT").ok().filter(|c| !c.is_empty() && c != "unknown");
        let host = std::env::var("HOSTNAME")
            .ok()
            .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty());
        Self { service: service.into(), version, commit, host }
    }

    /// The OpenTelemetry resource attributes besides `service.name`; an empty version is left out.
    pub fn attributes(&self) -> Vec<(&'static str, String)> {
        let os = std::env::consts::OS;
        let mut out = vec![("os.type", os.to_string())];
        out.extend((!self.version.is_empty()).then(|| ("service.version", self.version.to_string())));
        out.extend(self.commit.clone().map(|c| ("vcs.ref.head.revision", c)));
        out.extend(self.host.clone().map(|h| ("host.name", h)));
        out
    }
}

/// A call site: the file and line of the logging call, and the function around it when the `site!` macro made it.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub file: &'static str,
    pub line: u32,
    pub function: Option<&'static str>,
}

impl Site {
    /// The file and line of whoever called a `#[track_caller]` function; the function is unknown here, so `site!` is the
    /// complete form.
    #[track_caller]
    pub fn caller() -> Self {
        let at = std::panic::Location::caller();
        Self { file: at.file(), line: at.line(), function: None }
    }

    /// The OpenTelemetry code attributes.
    pub fn attributes(&self) -> Vec<(&'static str, serde_json::Value)> {
        let mut out = vec![("code.filepath", self.file.into()), ("code.lineno", self.line.into())];
        out.extend(self.function.map(|f| ("code.function", f.into())));
        out
    }
}

/// The name of the function a `type_name` path ends in: the path of the marker function `function_name!` declares inside it,
/// without that marker, closure or async-block segments, and without the module and type before the last `::`.
pub fn function_of(marker_path: &'static str) -> &'static str {
    let mut path = marker_path;
    while let Some(outer) = path.strip_suffix("::{{closure}}").or_else(|| path.strip_suffix("::__function_name_marker")) {
        path = outer;
    }
    path.rsplit("::").next().unwrap_or(path)
}
