use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const STUB_PNPM_OK: &str = "#!/bin/sh\nexit 0";
pub const STUB_EXPO_OK: &str = "#!/bin/sh\nexit 0";

/// Stubs every `with_stubs` call injects: tools that would change repository state, simulators or live processes if they ran for real.
fn default_stubs() -> HashMap<String, String> {
    [
        ("pnpm", STUB_PNPM_OK),
        ("expo", STUB_EXPO_OK),
        ("PlistBuddy", "#!/bin/sh\nexit 1"),
        ("defaults", "#!/bin/sh\nexit 1"),
        ("osascript", "#!/bin/sh\nexit 0"),
        ("open", "#!/bin/sh\nexit 0"),
        ("killall", "#!/bin/sh\nexit 0"),
        ("codesign", "#!/bin/sh\nexit 0"),
        // `lsof` finds nothing (exit 1), so no port holder is ever killed; `kill` is a no-op.
        ("lsof", "#!/bin/sh\nexit 1"),
        ("kill", "#!/bin/sh\nexit 0"),
    ]
    .into_iter()
    .map(|(name, body)| (name.to_string(), body.to_string()))
    .collect()
}

pub struct StubContext {
    pub bin_dir: PathBuf,
    pub cwd: PathBuf,
}

/// Runs `body` inside a temp tree holding the stub executables (the defaults, then `stubs`, which win) and an empty working
/// directory; the tree is removed afterwards, also when `body` panics.
pub fn with_stubs<R>(stubs: &[(&str, &str)], body: impl FnOnce(&StubContext) -> R) -> R {
    let base = tempfile::Builder::new().prefix("wf-test-").tempdir().expect("temp dir");
    let (bin_dir, cwd) = (base.path().join("bin"), base.path().join("cwd"));
    std::fs::create_dir_all(&bin_dir).expect("bin");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let mut merged = default_stubs();
    merged.extend(stubs.iter().map(|(name, content)| (name.to_string(), content.to_string())));
    for (name, content) in merged {
        let path = bin_dir.join(name);
        std::fs::write(&path, content).expect("stub");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    body(&StubContext { bin_dir, cwd })
}

/// Writes an executable bash stub: a `#!/bin/bash` line, the script and a trailing newline.
pub fn create_stub(path: &Path, script: &str) {
    std::fs::write(path, format!("#!/bin/bash\n{script}\n")).expect("stub");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// Writes an executable `sh` script: a `#!/bin/sh` line, the body and a trailing newline.
pub fn create_sh_stub(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("stub");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}
