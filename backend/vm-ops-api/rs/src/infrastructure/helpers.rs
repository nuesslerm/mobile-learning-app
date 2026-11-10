//! The three helper commands the sibling scripts call, as subcommands of this binary (the image of every sibling has no node): `provision`
//! fetches files from GitHub's raw host, `append-env-local` appends a `.env.local` to a synced `.env`, and `verify-env` refuses a
//! `.env` that would clobber what sync-env resolved. The rules are `core::domain::env_file`; this is the file and network I/O around them.
use std::io::Write;
use vm_ops_api_core::application::scripts::HELPER_SUBCOMMANDS;
use vm_ops_api_core::domain::env_file::{clobbered_keys, plan_append};

/// Whether `name` is one of the helper subcommands.
pub fn is_helper(name: &str) -> bool {
    HELPER_SUBCOMMANDS.contains(&name)
}

/// Runs the helper `name` with its arguments and returns its exit code. `token` is the provisioning token (`GITHUB_RAW_TOKEN`).
pub async fn run(name: &str, args: &[String], token: Option<&str>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    match name {
        "provision" => provision(args, token, stderr).await,
        "append-env-local" => append_env_local(args, stdout),
        "verify-env" => verify_env(args, stderr),
        _ => 2,
    }
}

fn say(out: &mut dyn Write, line: &str) {
    let _ = writeln!(out, "{line}");
}

/// A file read as Node reads it with `utf8`: a byte that is no UTF-8 is U+FFFD.
fn read_text(path: &str) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// `<url> <dest>` pairs, so no file list is ever spliced into program text. A GET that is not a 200 stops the run, files written so far stay.
async fn provision(args: &[String], token: Option<&str>, stderr: &mut dyn Write) -> i32 {
    if args.is_empty() || args.len() % 2 != 0 {
        say(stderr, "usage: vm-ops-api-rs provision <url> <dest> [<url> <dest>...]");
        return 2;
    }
    let client = reqwest::Client::new();
    for pair in args.chunks(2) {
        let (url, dest) = (&pair[0], &pair[1]);
        let mut request = client.get(url);
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            request = request.header("Authorization", format!("token {token}"));
        }
        let fetched = async {
            let response = request.send().await.map_err(|error| error.to_string())?;
            if response.status().as_u16() != 200 {
                return Err(format!("GET {url} -> {}", response.status().as_u16()));
            }
            let bytes = response.bytes().await.map_err(|error| error.to_string())?;
            std::fs::write(dest, &bytes).map_err(|error| error.to_string())
        };
        if let Err(message) = fetched.await {
            say(stderr, &format!("[provision] {message}"));
            return 1;
        }
    }
    0
}

/// Not `cat .env.local >> .env`: an empty `KEY=` there would beat the resolved secret above it.
fn append_env_local(args: &[String], stdout: &mut dyn Write) -> i32 {
    let [env_path, env_local_path, ..] = args else {
        say(stdout, "usage: vm-ops-api-rs append-env-local <env-file> <env-local-file>");
        return 2;
    };
    if !std::path::Path::new(env_local_path).exists() {
        return 0;
    }
    let (env, local) = match (read_text(env_path), read_text(env_local_path)) {
        (Ok(env), Ok(local)) => (env, local),
        (Err(error), _) | (_, Err(error)) => {
            say(stdout, &format!("[append-env-local] {error}"));
            return 1;
        }
    };
    let plan = plan_append(&env, &local);
    if !plan.dropped.is_empty() {
        say(stdout, &format!("[append-env-local] ignored {} empty override(s) that would have shadowed a resolved value: {}", plan.dropped.len(), plan.dropped.join(", ")));
    }
    if !plan.keep.is_empty() {
        let appended = std::fs::OpenOptions::new().append(true).open(env_path).and_then(|mut file| file.write_all(format!("{}\n", plan.keep.join("\n")).as_bytes()));
        if let Err(error) = appended {
            say(stdout, &format!("[append-env-local] {error}"));
            return 1;
        }
    }
    say(stdout, &format!("[append-env-local] appended {} line(s) from {env_local_path}", plan.keep.len()));
    0
}

/// Runs before `compose up`; a non-zero exit leaves the running containers untouched. Prints key names only, never a value.
fn verify_env(args: &[String], stderr: &mut dyn Write) -> i32 {
    let [synced_path, final_path, ..] = args else {
        say(stderr, "usage: vm-ops-api-rs verify-env <synced-snapshot> <final-env>");
        return 2;
    };
    let (synced, last) = match (read_text(synced_path), read_text(final_path)) {
        (Ok(synced), Ok(last)) => (synced, last),
        (Err(error), _) | (_, Err(error)) => {
            say(stderr, &format!("[verify-env] {error}"));
            return 1;
        }
    };
    let found = clobbered_keys(&synced, &last);
    if !found.clobbered.is_empty() {
        say(stderr, &format!("[verify-env] FAILED: {} key(s) resolved by sync-env would reach the container empty: {}", found.clobbered.len(), found.clobbered.join(", ")));
        say(stderr, "[verify-env] Something appended after sync-env (HOST_LABEL, .env.local) overrode them. Compose takes the LAST occurrence.");
    } else if !found.mangled.is_empty() {
        say(stderr, &format!("[verify-env] FAILED: {} key(s) are JSON-quoted twice, so the app would receive the quotes as part of the secret: {}", found.mangled.len(), found.mangled.join(", ")));
        say(stderr, "[verify-env] Check this .env for a hand-appended pre-quoted line, .env.local, and any ambient env var of the same name.");
    } else {
        say(stderr, &format!("[verify-env] ok: {} synced key(s) survive into the final .env", found.synced_count));
        return 0;
    }
    say(stderr, "[verify-env] Refusing to restart containers — the running ones are left untouched.");
    1
}
