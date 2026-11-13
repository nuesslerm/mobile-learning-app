//! `wf ship build <app> --tag <tag>` and `wf ship promote <app> --tag <tag>`: builds the images of an app (and the sidecars its compose pulls on the same tag) with the docker CLI
//! against whatever `DOCKER_HOST` names, and pushes them. On a hosted runner that is the local
//! daemon; the `build-image` action calls it. The registry credential is read from `DOCKERHUB_TOKEN`, given to `docker login` on stdin only.
use crate::build_image as bi;
use serde_json::{json, Value};
use std::io::Write;
use std::os::fd::AsFd;
use std::process::{Command, Stdio};
use wf_kit::args::parse;

const NAMESPACE: &str = "nuesslerm";
const BUILDER: &str = "ci-builder";
const DEFAULT_PLATFORM: &str = "linux/arm64";

/// The docker CLI as `wf` runs it: arguments, and text for stdin when a command reads it. The result is the exit code.
pub trait Docker {
    fn run(&self, args: &[String], stdin: Option<&str>) -> Result<i32, String>;
}

struct DockerCli;

impl Docker for DockerCli {
    fn run(&self, args: &[String], stdin: Option<&str>) -> Result<i32, String> {
        // stdout joins stderr so the verb's own stdout stays the one JSON line
        let out = std::io::stderr().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
        let mut child = Command::new("docker")
            .args(args)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::from(out))
            .spawn()
            .map_err(|e| format!("docker could not start: {e}"))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes()).map_err(|e| format!("docker closed its stdin: {e}"))?;
        }
        child.wait().map(|s| s.code().unwrap_or(1)).map_err(|e| e.to_string())
    }
}

struct Build<'a> {
    app: &'a str,
    tag: &'a str,
    commit: &'a str,
    platform: &'a str,
    /// `(image, content hash)` in build order.
    images: Vec<(String, String)>,
    login: (&'a str, &'a str),
    /// Export the layer cache to the registry too (`--cache-push`: a push to dev only; every build reads it).
    cache_push: bool,
    /// `--no-push`: build every image from scratch for the measurement and push nothing (no tag, no cache export, no re-tag of a reused hash).
    no_push: bool,
    /// `--fresh`: rebuild every image with no layer cache and no reuse of an image of the same hash, and push it as usual.
    fresh: bool,
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

fn ok(docker: &dyn Docker, args: &[String], stdin: Option<&str>) -> Result<(), String> {
    match docker.run(args, stdin)? {
        0 => Ok(()),
        code => Err(format!("docker {} failed with exit code {code}", args.iter().take(2).cloned().collect::<Vec<_>>().join(" "))),
    }
}

const BUILD_ATTEMPTS: u32 = 4;

/// A build resolves its base images from the registry inside the buildx builder, which a pre-pull into the daemon store cannot reach, so a registry blip
/// (a 504 on a pinned FROM) is survived by running the whole build again; the layer cache makes a retry cheap. Backoff is 15 s, 30 s, 45 s
/// (`WF_BUILD_RETRY_DELAY_MS` overrides the step), and the last attempt's error is the verb's.
fn ok_retrying(docker: &dyn Docker, args: &[String], stdin: Option<&str>) -> Result<(), String> {
    let step: u64 = std::env::var("WF_BUILD_RETRY_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(15_000);
    let mut attempt = 1;
    loop {
        match ok(docker, args, stdin) {
            Err(e) if attempt < BUILD_ATTEMPTS => {
                eprintln!("[ci-build] attempt {attempt}/{BUILD_ATTEMPTS} failed ({e}); retrying");
                std::thread::sleep(std::time::Duration::from_millis(step * attempt as u64));
                attempt += 1;
            }
            result => return result,
        }
    }
}

/// Logs in (the credential on stdin only) and selects the builder, creating it on the first call of a job.
fn prepare(docker: &dyn Docker, login: (&str, &str)) -> Result<(), String> {
    ok(docker, &strings(&["login", "-u", login.0, "--password-stdin"]), Some(login.1))?;
    if docker.run(&strings(&["buildx", "inspect", BUILDER]), None)? != 0 {
        ok(docker, &strings(&["buildx", "create", "--name", BUILDER, "--driver", "docker-container", "--bootstrap"]), None)?;
    }
    ok(docker, &strings(&["buildx", "use", BUILDER]), None)
}

fn run_promote(docker: &dyn Docker, app: &str, tag: &str, images: &[&str], login: (&str, &str)) -> Result<Value, String> {
    prepare(docker, login)?;
    let mut promoted = Vec::new();
    for image in images {
        let reference = format!("docker.io/{NAMESPACE}/{image}");
        ok(docker, &strings(&["buildx", "imagetools", "create", "-t", &format!("{reference}:latest"), &format!("{reference}:{tag}")]), None)?;
        eprintln!("[ci-build] {reference}:latest -> {tag}");
        promoted.push(json!({ "image": image, "latest": format!("{reference}:latest"), "from": format!("{reference}:{tag}") }));
    }
    Ok(json!({ "app": app, "tag": tag, "images": promoted }))
}

fn run_build(docker: &dyn Docker, b: &Build) -> Result<Value, String> {
    prepare(docker, b.login)?;
    let mut built = Vec::new();
    for (image, hash) in &b.images {
        let spec = pipeline::image_hash::IMAGES.iter().find(|i| i.name == image).ok_or_else(|| format!("{image} has no build spec"))?;
        let reference = format!("docker.io/{NAMESPACE}/{image}");
        let existing = format!("{reference}:nx-{hash}");
        let reused = !b.no_push && !b.fresh && docker.run(&strings(&["buildx", "imagetools", "inspect", &existing]), None)? == 0;
        let tagged = format!("{reference}:{}", b.tag);
        if reused {
            // a one-line layer on the image of this hash names the commit the tag is for
            let layer = format!("FROM {existing}\nENV GIT_COMMIT={}\n", b.commit);
            ok_retrying(docker, &strings(&["buildx", "build", "--platform", b.platform, "-t", &tagged, "--push", "-"]), Some(&layer))?;
        } else {
            let cache = format!("type=registry,ref={reference}:buildcache");
            let mut args = strings(&[
                "buildx", "build", "--platform", b.platform,
                "--build-arg", &format!("GIT_COMMIT={}", b.commit),
                "--label", &format!("wf.input-hash={hash}"),
                "-f", spec.dockerfile, "-t", &tagged, "-t", &existing,
            ]);
            args.extend(if b.fresh { strings(&["--no-cache"]) } else { strings(&["--cache-from", &cache]) });
            if b.cache_push && !b.no_push {
                args.extend(strings(&["--cache-to", &format!("{cache},mode=max")]));
            }
            args.extend(strings(if b.no_push { &["--output", "type=cacheonly"] } else { &["--push"] }));
            args.push(spec.context.to_string());
            ok_retrying(docker, &args, None)?;
        }
        if b.no_push {
            eprintln!("[ci-build] built {tagged} (not pushed)");
        } else {
            eprintln!("[ci-build] pushed {tagged}{}", if reused { " (unchanged, re-tagged with the commit)" } else { "" });
        }
        built.push(json!({ "image": image, "ref": tagged, "reused": reused, "pushed": !b.no_push }));
    }
    eprintln!("[ci-build] done tag={}", b.tag);
    Ok(json!({ "app": b.app, "tag": b.tag, "commit": b.commit, "images": built }))
}

pub fn run(args: &[String]) -> Result<Option<i32>, String> {
    let Some(p) = parse(args, &["tag", "ref", "platform"], &["cache-push", "images", "no-push", "fresh"]) else { return Ok(None) };
    let [app] = p.positional.as_slice() else { return Err("ship build takes exactly one <app>".into()) };
    let names = pipeline::image_hash::images_of(app);
    if p.flags.iter().any(|f| f == "images") {
        println!("{}", json!({ "app": app, "images": names }));
        return Ok(Some(0));
    }
    let tag = p.val("tag").ok_or("ship build needs --tag <tag>")?;
    let git_ref = p.val("ref").unwrap_or("HEAD");
    bi::check(git_ref, Some(tag))?;
    if names.is_empty() {
        return Err(format!("{app} builds no image"));
    }
    let commit = crate::ship::full_commit(git_ref)?;
    let images = names.iter().map(|n| pipeline::image_hash::hash(None, git_ref, n).map(|h| (n.to_string(), h))).collect::<Result<Vec<_>, _>>()?;
    let token = std::env::var("DOCKERHUB_TOKEN").ok().filter(|t| !t.is_empty()).ok_or("DOCKERHUB_TOKEN is not set")?;
    let user = std::env::var("DOCKERHUB_USERNAME").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| NAMESPACE.to_string());
    let build = Build { app, tag, commit: &commit, platform: p.val("platform").unwrap_or(DEFAULT_PLATFORM), images, login: (&user, &token), cache_push: p.flags.iter().any(|f| f == "cache-push"), no_push: p.flags.iter().any(|f| f == "no-push"), fresh: p.flags.iter().any(|f| f == "fresh") };
    let result = run_build(&DockerCli, &build)?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(Some(0))
}

/// `wf ship promote <app> --tag <tag>`: each image of the app's `latest` is pointed at `tag`, registry to registry.
pub fn promote(args: &[String]) -> Result<Option<i32>, String> {
    let Some(p) = parse(args, &["tag"], &[]) else { return Ok(None) };
    let [app] = p.positional.as_slice() else { return Err("ship promote takes exactly one <app>".into()) };
    let tag = p.val("tag").ok_or("ship promote needs --tag <tag>")?;
    bi::check("HEAD", Some(tag))?;
    let names = pipeline::image_hash::images_of(app);
    if names.is_empty() {
        return Err(format!("{app} builds no image"));
    }
    let token = std::env::var("DOCKERHUB_TOKEN").ok().filter(|t| !t.is_empty()).ok_or("DOCKERHUB_TOKEN is not set")?;
    let user = std::env::var("DOCKERHUB_USERNAME").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| NAMESPACE.to_string());
    let result = run_promote(&DockerCli, app, tag, &names, (&user, &token))?;
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(Some(0))
}
