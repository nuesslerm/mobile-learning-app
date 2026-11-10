//! What the registry-side verbs share: the image names `wf ship` knows, the safe-input check, and the `sh` script that moves `latest`
//! (`--promote-latest`, run inside agent-shell). Pure: no I/O. Images are built by CI (`ci_build.rs`), never here.
use regex::Regex;
use pipeline::image_hash::IMAGES;

pub fn is_buildable(name: &str) -> bool {
    IMAGES.iter().any(|i| i.name == name)
}

const DOCKER_LOGIN: &str = r#": "${DOCKERHUB_TOKEN:?DOCKERHUB_TOKEN is not set}"; printf %s "$DOCKERHUB_TOKEN" | docker login -u "${DOCKERHUB_USERNAME:-nuesslerm}" --password-stdin >/dev/null || { echo "[vm-build] docker login failed" >&2; exit 1; }; echo "[vm-build] docker login ok""#;

fn js_json_string(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

/// An `Err` is the TypeScript's plain `Error` (unsafe input).
pub fn check(git_ref: &str, tag: Option<&str>) -> Result<(), String> {
    let safe_ref = Regex::new(r"^[A-Za-z0-9._/-]+$").unwrap();
    let safe_tag = Regex::new(r"^[A-Za-z0-9._-]{1,128}$").unwrap();
    if !safe_ref.is_match(git_ref) {
        return Err(format!("unsafe ref: {}", js_json_string(git_ref)));
    }
    match tag {
        Some(tag) if !safe_tag.is_match(tag) => Err(format!("unsafe tag: {}", js_json_string(tag))),
        _ => Ok(()),
    }
}

/// Points each image's `latest` at `tag`, registry to registry, so a recreated VM pulls a build that served. Any failure
/// fails the job.
pub fn promote_script(images: &[String], tag: &str) -> Result<String, String> {
    if !Regex::new(r"^[A-Za-z0-9._-]{1,128}$").unwrap().is_match(tag) {
        return Err(format!("unsafe tag: {}", js_json_string(tag)));
    }
    if images.is_empty() {
        return Err("no images to promote".into());
    }
    let mut lines: Vec<String> = vec!["set -eu".into(), DOCKER_LOGIN.into()];
    for image in images {
        let reference = format!("docker.io/nuesslerm/{image}");
        lines.push(format!("docker buildx imagetools create -t \"{reference}:latest\" \"{reference}:{tag}\""));
        lines.push(format!("echo \"[vm-build] {reference}:latest -> {tag}\""));
    }
    lines.push(format!("echo \"[vm-build] promoted tag={tag}\""));
    Ok(format!("{}\n", lines.join("\n")))
}
