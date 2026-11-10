use super::support::*;

fn consumer(repo: &Repo, deps: &str) -> std::path::PathBuf {
    let dir = repo.dir.path().join("consumer");
    write(&dir.join("Cargo.toml"), &crate_manifest("consumer", "0.1.0", deps));
    write(&dir.join("src/main.rs"), "fn main() { println!(\"{}\", fixture_core::answer()); }\n");
    dir.join("Cargo.toml")
}

const CRATE: &str = "libs/crates/fixture-core/0.1.1.crate";
const INDEX: &str = "libs/index/fi/xt/fixture-core";

#[test]
fn a_released_crate_and_its_index_land_in_the_one_private_bucket_and_nothing_names_a_link() {
    // Given a library at 0.1.0 and an empty bucket
    let repo = Repo::new(&libraries_json("mla", "", ""));

    // When it is released as a patch
    let r = repo.lib(&["release", "fixture-core", "patch"]);

    // Then the version is raised, and the crate, its index file and config.json are all under libs/ of the same bucket
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!((out["ok"].clone(), out["version"].as_str(), out["previous"].as_str(), out["publisher"].as_str()), (true.into(), Some("0.1.1"), Some("0.1.0"), Some("bucket-sparse")));
    assert_eq!(out["objects"], serde_json::json!(["crates/fixture-core/0.1.1.crate", "index/fi/xt/fixture-core", "index/config.json"]));
    assert!(repo.read("libs/fixture-core/Cargo.toml").contains("version = \"0.1.1\""));
    assert_eq!(repo.bucket.names(), [CRATE, "libs/index/config.json", INDEX]);
    let line: serde_json::Value = serde_json::from_str(repo.bucket.text(INDEX).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!((line["name"].as_str(), line["vers"].as_str(), line["yanked"].as_bool()), (Some("fixture-core"), Some("0.1.1"), Some(false)));
    assert_eq!(line["cksum"].as_str(), Some(sha256_hex(&repo.bucket.get(CRATE).unwrap()).as_str()));
    // And no output or stored index file carries the bucket link
    assert!(!r.stdout.contains("SECRET-PAR-TOKEN") && !r.stderr.contains("SECRET-PAR-TOKEN"));
    assert!(repo.bucket.names().iter().filter(|n| !n.ends_with(".crate")).all(|n| !repo.bucket.text(n).unwrap().contains("/p/")));
}

#[test]
fn config_json_sends_downloads_through_the_proxy_and_is_the_same_bytes_the_proxy_generates() {
    // Given a release
    let repo = Repo::new(&libraries_json("mla", "", ""));
    assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);

    // When config.json is read from the bucket and from the proxy
    let stored = repo.bucket.text("libs/index/config.json").unwrap();
    let (status, served) = repo.proxy.fetch("/index/config.json");

    // Then it is exactly {"dl": <proxy>/dl/{crate}/{version}.crate} with no api field, and both agree byte for byte
    assert_eq!(stored, format!("{{\"dl\":\"{}/dl/{{crate}}/{{version}}.crate\"}}\n", repo.proxy.base()));
    assert_eq!((status, served), (200, stored.into_bytes()));
}

#[test]
fn a_consumers_cargo_resolves_through_the_proxy_downloads_the_crate_from_its_dl_route_and_locks_no_link() {
    // Given a released library and a consumer that resolves it from the registry
    let repo = Repo::new(&libraries_json("mla", "", ""));
    assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);
    let manifest = consumer(&repo, "fixture-core = { version = \"0.1.1\", registry = \"mla\" }\n");

    // When cargo resolves and builds it
    assert_eq!(repo.cargo_run(&manifest, &[("CARGO_REGISTRIES_MLA_INDEX", &repo.proxy_index())]), "42");

    // Then the consumer's Cargo.lock names the proxy's index as the source and carries no PAR token
    let lock = std::fs::read_to_string(manifest.with_file_name("Cargo.lock")).unwrap();
    assert!(lock.contains(&format!("source = \"{}\"", repo.proxy_index())), "{lock}");
    assert!(!lock.contains("/p/") && !lock.contains("SECRET-PAR-TOKEN"), "{lock}");
    // And the crate bytes came through the proxy's /dl route, the index through /index
    let asked = repo.proxy.requests();
    assert!(asked.contains(&"/dl/fixture-core/0.1.1.crate".to_string()) && asked.contains(&"/index/config.json".to_string()) && asked.contains(&"/index/fi/xt/fixture-core".to_string()), "{asked:?}");
}

#[test]
fn a_release_needs_only_the_write_link_and_the_proxy() {
    // Given an environment with the write link and no read link (a CI job on the tailnet)
    let repo = Repo::new(&libraries_json("mla", "", ""));
    let mut opts = repo.opts();
    opts.env.remove("LIBS_READ_PAR_URL");

    // When a library is released twice
    let first = repo.lib_with(&["release", "fixture-core", "patch"], opts);
    let second = repo.lib(&["release", "fixture-core", "patch"]);

    // Then both succeed and the second index file lists both versions, read through the proxy before it was appended
    assert_eq!((first.exit_code, second.exit_code), (0, 0), "{}{}{}{}", first.stdout, first.stderr, second.stdout, second.stderr);
    assert_eq!(repo.bucket.text(INDEX).unwrap().lines().count(), 2);
    assert!(repo.proxy.requests().contains(&"/index/fi/xt/fixture-core".to_string()));
}

#[test]
fn a_version_already_in_the_index_is_refused_even_when_its_crate_is_missing() {
    // Given an index that already lists 0.1.1 (its crate not in the bucket)
    let repo = Repo::new(&libraries_json("mla", "", ""));
    repo.bucket.put(INDEX, b"{\"name\":\"fixture-core\",\"vers\":\"0.1.1\",\"deps\":[],\"cksum\":\"x\",\"features\":{},\"yanked\":false}\n");

    // When 0.1.1 is released
    let r = repo.lib(&["release", "fixture-core", "patch"]);

    // Then it is refused as already released and no crate is uploaded
    assert_eq!(r.exit_code, 1);
    assert!(last_json(&r)["error"].as_str().unwrap().contains("already released"), "{}", r.stdout);
    assert_eq!(repo.bucket.names(), [INDEX]);
}

#[test]
fn a_release_with_the_proxy_down_stops_before_uploading_anything_and_says_what_to_do() {
    // Given a registry whose proxy nothing answers on and no read link
    let repo = Repo::new(&libraries_json("mla", "", "").replace("@INDEX@", "http://127.0.0.1:1/index/"));
    let mut opts = repo.opts();
    opts.env.remove("LIBS_READ_PAR_URL");

    // When a library is released
    let r = repo.lib_with(&["release", "fixture-core", "patch"], opts);

    // Then it fails with a next step, the manifest is back at 0.1.0 and the bucket is empty
    assert_eq!(r.exit_code, 1);
    assert!(last_json(&r)["next"].as_str().unwrap().contains("proxy"), "{}", r.stdout);
    assert!(repo.read("libs/fixture-core/Cargo.toml").contains("version = \"0.1.0\""));
    assert!(repo.bucket.names().is_empty());
}

#[test]
fn a_version_already_in_the_bucket_is_refused_and_nothing_is_changed() {
    // Given 0.1.1 released, and a checkout still at 0.1.0 (another branch released first)
    let repo = Repo::new(&libraries_json("mla", "", ""));
    assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);
    let before = repo.bucket.get(CRATE).unwrap();
    repo.core("0.1.0", "pub fn answer() -> u32 { 7 }\n");

    // When the same version is released again
    let r = repo.lib(&["release", "fixture-core", "patch"]);

    // Then it is refused with a next step, the manifest is untouched and the stored crate is the first one
    assert_eq!(r.exit_code, 1);
    let out = last_json(&r);
    assert!(out["ok"] == false && out["error"].as_str().unwrap().contains("already released") && out["next"].is_string(), "{out}");
    assert!(repo.read("libs/fixture-core/Cargo.toml").contains("version = \"0.1.0\""));
    assert_eq!(repo.bucket.get(CRATE).unwrap(), before);
}

#[test]
fn a_crate_that_does_not_package_leaves_the_manifest_and_the_bucket_as_they_were() {
    // Given a library whose source does not compile
    let repo = Repo::new(&libraries_json("mla", "", ""));
    repo.core("0.1.0", "pub fn answer() -> u32 { not rust }\n");

    // When it is released
    let r = repo.lib(&["release", "fixture-core", "minor"]);

    // Then it fails with cargo's reason, the version is restored and nothing was uploaded
    assert_eq!(r.exit_code, 1);
    let out = last_json(&r);
    assert!(out["ok"] == false && out["error"].as_str().unwrap().contains("cargo package failed") && out["next"].is_string(), "{out}");
    assert!(repo.read("libs/fixture-core/Cargo.toml").contains("version = \"0.1.0\""));
    assert!(repo.bucket.names().is_empty());
}

#[test]
fn a_library_that_depends_on_another_released_library_resolves_both_through_its_index_entry() {
    // Given fixture-core released and fixture-app depending on it from the registry
    let libs = r#",
    { "name": "fixture-app", "kind": "rust", "path": "libs/fixture-app", "registry": "mla" }"#;
    let repo = Repo::new(&libraries_json("mla", "", libs));
    assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);
    repo.write("libs/fixture-app/Cargo.toml", &crate_manifest("fixture-app", "0.1.0", "fixture-core = { version = \"0.1.1\", registry = \"mla\", path = \"../fixture-core\" }\n"));
    repo.write("libs/fixture-app/src/lib.rs", "pub fn answer() -> u32 { fixture_core::answer() }\n");

    // When fixture-app is released
    let r = repo.lib(&["release", "fixture-app", "patch"]);

    // Then its index line names the dependency on this registry
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let line: serde_json::Value = serde_json::from_str(repo.bucket.text("libs/index/fi/xt/fixture-app").unwrap().lines().next().unwrap()).unwrap();
    assert_eq!((line["deps"][0]["name"].as_str(), line["deps"][0]["req"].as_str(), line["deps"][0]["registry"].is_null()), (Some("fixture-core"), Some("^0.1.1"), true));

    // And a consumer of fixture-app alone builds, with cargo fetching fixture-core by that entry
    let dir = repo.dir.path().join("consumer");
    write(&dir.join("Cargo.toml"), &crate_manifest("consumer", "0.1.0", "fixture-app = { version = \"0.1.1\", registry = \"mla\" }\n"));
    write(&dir.join("src/main.rs"), "fn main() { println!(\"{}\", fixture_app::answer()); }\n");
    assert_eq!(repo.cargo_run(&dir.join("Cargo.toml"), &[("CARGO_REGISTRIES_MLA_INDEX", &repo.proxy_index())]), "42");
}

#[test]
fn a_registry_with_the_cargo_publish_api_receives_the_crate_with_its_own_token_and_the_consumer_builds_from_it() {
    // Given a library whose registry is another sparse registry with a publish API, and a token in the environment
    let fake = CargoRegistryFake::start();
    let extra = format!(r#",
    {{ "alias": "hosted", "kind": "cargo-sparse", "url": "{}", "tokenEnv": "HOSTED_TOKEN", "publish": "cargo-publish" }}"#, fake.index_url());
    let mut repo = Repo::new(&libraries_json("hosted", &extra, ""));
    repo.env.push(("HOSTED_TOKEN".into(), "hosted-secret-value".into()));

    // When it is released
    let r = repo.lib(&["release", "fixture-core", "patch"]);

    // Then that registry was sent fixture-core 0.1.1 with the token, nothing went to the bucket and the token is not printed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(fake.received().iter().map(|x| (x.name.as_str(), x.vers.as_str(), x.token.as_deref())).collect::<Vec<_>>(), [("fixture-core", "0.1.1", Some("hosted-secret-value"))]);
    assert_eq!(last_json(&r)["publisher"], "cargo-publish");
    assert!(repo.bucket.names().is_empty());
    assert!(!r.stdout.contains("hosted-secret-value") && !r.stderr.contains("hosted-secret-value"));

    // And a consumer naming that alias builds from it: the same release, a different registry kind
    let manifest = consumer(&repo, "fixture-core = { version = \"0.1.1\", registry = \"hosted\" }\n");
    assert_eq!(repo.cargo_run(&manifest, &[("CARGO_REGISTRIES_HOSTED_INDEX", &fake.index_url())]), "42");
}

#[test]
fn a_registry_without_a_publish_adapter_or_without_its_token_is_refused_with_a_next_step() {
    // Given a read-only registry and a cargo-publish registry whose token variable is unset
    let extra = r#",
    { "alias": "ro", "kind": "cargo-sparse", "url": "sparse+http://127.0.0.1:1/", "tokenEnv": null, "publish": null },
    { "alias": "tokenless", "kind": "cargo-sparse", "url": "sparse+http://127.0.0.1:1/", "tokenEnv": "NO_SUCH_TOKEN_VARIABLE", "publish": "cargo-publish" }"#;
    for (alias, said) in [("ro", "read-only"), ("tokenless", "NO_SUCH_TOKEN_VARIABLE")] {
        let repo = Repo::new(&libraries_json(alias, extra, ""));

        // When the library is released
        let r = repo.lib(&["release", "fixture-core", "patch"]);

        // Then the refusal names the reason and the manifest is untouched
        assert_eq!(r.exit_code, 1, "{alias}");
        assert!(last_json(&r)["error"].as_str().unwrap().contains(said), "{alias}: {}", r.stdout);
        assert!(repo.read("libs/fixture-core/Cargo.toml").contains("version = \"0.1.0\""));
    }
}
