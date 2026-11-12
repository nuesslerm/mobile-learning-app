//! Putting a `wf` build onto the VM: the release asset CI published is fetched by a sibling that checks it against the sha256 the caller
//! (CI, which built it) names, then installs it on the `clis` volume under its tree hash and points `current` at it.

/// The public release CI publishes the VM's `wf` build to; the VM is arm64.
pub const ASSET_URL: &str = "https://github.com/nuesslerm/mobile-learning-app/releases/download/wf-latest/wf-linux-arm64.tar.gz";
const VOLUME: &str = "clis:/opt/clis";
/// The newest builds kept beside the one `current` names.
const KEEP: usize = 4;

/// A SHA-256 as `sha256sum` prints it: 64 lowercase hex digits, nothing else, so it can be spliced into the sibling's script.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The script the sibling runs: download, check, install under the tree hash, flip `current`, prune. It prints the tree hash last.
pub fn script(sha256: &str) -> String {
    format!(
        "set -e\n\
d=$(mktemp -d /opt/clis/.install-XXXXXX)\n\
vm-ops-api-rs provision '{ASSET_URL}' \"$d/a.tgz\"\n\
[ \"$(sha256sum \"$d/a.tgz\" | cut -d' ' -f1)\" = '{sha256}' ] || {{ echo 'the release asset is not the build the caller named (sha256 differs)' >&2; rm -rf \"$d\"; exit 1; }}\n\
mkdir \"$d/x\" && tar -xzf \"$d/a.tgz\" -C \"$d/x\" && chmod 755 \"$d/x\" \"$d\"/x/*\n\
tree=$(\"$d/x/wf\" build-sha)\n\
[ -n \"$tree\" ] || {{ echo 'the installed wf printed no build-sha' >&2; rm -rf \"$d\"; exit 1; }}\n\
[ -x \"/opt/clis/$tree/wf\" ] || mv -T \"$d/x\" \"/opt/clis/$tree\"\n\
ln -sfn \"$tree\" /opt/clis/.current-new && mv -T /opt/clis/.current-new /opt/clis/current\n\
rm -rf \"$d\"\n\
cur=$(readlink /opt/clis/current)\n\
ls -1dt /opt/clis/*/ | tail -n +{} | while read -r old; do [ \"$(basename \"$old\")\" = \"$cur\" ] || rm -rf \"$old\"; done\n\
echo \"$tree\"\n",
        KEEP + 1
    )
}

/// `docker run` of the sibling: waited for, removed when done, on this service's own image, with the `clis` volume writable.
pub fn sibling_args(image: &str, sha256: &str) -> Vec<String> {
    ["run", "--pull", "missing", "--rm", "-v", VOLUME, image, "sh", "-c", &script(sha256)].map(str::to_string).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn given_texts_when_checked_then_only_64_lowercase_hex_digits_are_a_sha256() {
        assert!(valid_sha256(SHA));
        for bad in ["", "abc", &SHA[1..], &format!("{SHA}0"), &SHA.to_uppercase(), &format!("{};id", &SHA[..56]), "g123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"] {
            assert!(!valid_sha256(bad), "{bad:?}");
        }
    }

    #[test]
    fn given_a_sha256_when_the_script_is_rendered_then_it_checks_that_hash_of_the_fixed_asset_and_names_the_tree_last() {
        let script = script(SHA);

        assert!(script.contains(&format!("provision '{ASSET_URL}'")), "{script}");
        assert!(script.contains(&format!("= '{SHA}'")), "{script}");
        assert!(script.trim_end().ends_with("echo \"$tree\""), "{script}");
        assert!(script.contains("tail -n +5"), "keeps the newest four: {script}");
    }

    #[test]
    fn given_the_sibling_when_rendered_then_it_waits_and_mounts_only_the_clis_volume_and_no_docker_socket() {
        let args = sibling_args("ns/vm-ops-api-rs:sha-1", SHA);

        assert_eq!(args[..8], ["run", "--pull", "missing", "--rm", "-v", "clis:/opt/clis", "ns/vm-ops-api-rs:sha-1", "sh"]);
        assert!(!args.iter().any(|a| a.contains("docker.sock") || a == "-d"));
    }
}
