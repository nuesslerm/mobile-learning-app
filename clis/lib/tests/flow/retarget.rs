use super::support::*;

const APP_BARE: &str = "fixture-core = \"0.1.1\"\n";

fn with_app(registry_extra: &str) -> Repo {
    let repo = Repo::new(&libraries_json("mla", registry_extra, ""));
    repo.write("services/app/Cargo.toml", &format!("{}\n[dev-dependencies]\nfixture-core = {{ version = \"0.1.1\", features = [] }}\n", crate_manifest("app", "0.1.0", APP_BARE).trim_end()));
    repo.write("services/app/src/main.rs", "fn main() { println!(\"{}\", fixture_core::answer()); }\n");
    repo.write("services/pathy/Cargo.toml", &crate_manifest("pathy", "0.1.0", "fixture-core = { path = \"../../libs/fixture-core\" }\n"));
    repo.commit("consumers");
    repo
}

#[test]
fn retargeting_to_the_bucket_and_to_crates_io_rewrites_the_dependency_lines_and_the_same_release_builds_either_way() {
    // Given a released library and a consumer on the crates.io line shape
    let repo = with_app("");
    repo.settle();
    assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);
    let app = repo.work().join("services/app/Cargo.toml");

    // When it is retargeted to the bucket registry
    let r = repo.lib(&["retarget", "fixture-core", "mla"]);

    // Then both dependency lines name the alias, the path dependency is left and listed, and libraries.json records it
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!(out["changed"].as_array().unwrap().len(), 2);
    assert!(out["skipped"][0]["manifest"].as_str().unwrap().ends_with("pathy/Cargo.toml"), "{out}");
    let text = std::fs::read_to_string(&app).unwrap();
    assert!(text.contains("fixture-core = { version = \"0.1.1\", registry = \"mla\" }") && text.contains("features = [], registry = \"mla\""), "{text}");
    assert!(repo.read("libraries.json").contains("\"registry\": \"mla\""));
    // And the public bucket's index is written literally into the workspace config, with no link in it
    assert_eq!(out["configs"][0]["written"], true);
    let config = repo.read("services/app/.cargo/config.toml");
    assert!(config.contains("[registries.mla]") && config.contains(&format!("index = \"{}\"", repo.proxy_index())) && !config.contains("/p/"), "{config}");

    // And the consumer builds from the bucket's files
    assert_eq!(repo.cargo_run(&app, &[("CARGO_REGISTRIES_MLA_INDEX", &repo.proxy_index())]), "42");

    // When it is retargeted to crates.io
    let r = repo.lib(&["retarget", "fixture-core", "crates-io"]);

    // Then the lines are back to the bare crates.io shape (the line of a bare version is a plain string again)
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let text = std::fs::read_to_string(&app).unwrap();
    assert!(text.contains("fixture-core = \"0.1.1\"") && !text.contains("registry"), "{text}");

    // And the unchanged release builds from that shape too, with the crates.io source standing in as the same sparse files
    let replaced = ["source.crates-io.replace-with=\"bucket\"".to_string(), format!("source.bucket.registry=\"{}\"", repo.proxy_index())];
    assert_eq!(repo.cargo_run_with(&app, &[], &replaced), "42");
}

#[test]
fn a_registry_with_a_literal_url_gets_its_index_written_to_the_workspace_cargo_config_and_a_second_run_changes_nothing() {
    // Given another registry in libraries.json with a plain url, and a consumer with a .cargo/config.toml of its own
    let extra = r#",
    { "alias": "hosted", "kind": "cargo-sparse", "url": "https://registry.example.com/index/", "tokenEnv": "HOSTED_TOKEN", "publish": "cargo-publish" }"#;
    let repo = with_app(extra);
    repo.write("services/app/.cargo/config.toml", "# keep me\n[build]\njobs = 2\n");
    repo.commit("config");

    // When the library is retargeted to it, twice
    let first = repo.lib(&["retarget", "fixture-core", "hosted"]);
    let config = repo.read("services/app/.cargo/config.toml");
    let second = repo.lib(&["retarget", "fixture-core", "hosted"]);

    // Then the index is in the config next to what was already there, the lines name the alias, and the second run changes no line
    assert_eq!(first.exit_code, 0, "{}{}", first.stdout, first.stderr);
    assert!(config.contains("# keep me") && config.contains("jobs = 2") && config.contains("[registries.hosted]") && config.contains("index = \"sparse+https://registry.example.com/index/\""), "{config}");
    assert!(repo.read("services/app/Cargo.toml").contains("registry = \"hosted\""));
    assert_eq!(last_json(&second)["changed"].as_array().unwrap().len(), 0);
    assert_eq!(repo.read("services/app/.cargo/config.toml"), config);
}

#[test]
fn a_registry_whose_index_is_a_credential_gets_no_config_file_and_the_output_says_how_to_build() {
    // Given a registry with no indexUrl whose url is an environment link
    let extra = r#",
    { "alias": "linked", "kind": "cargo-sparse", "url": "env:LIBS_READ_PAR_URL", "tokenEnv": null, "publish": null }"#;
    let repo = with_app(extra);

    // When the library is retargeted to it
    let r = repo.lib_with(&["retarget", "fixture-core", "linked"], repo.opts().env("LIBS_READ_PAR_URL", &repo.bucket.par_url()));

    // Then nothing is written and the output names the variable to build with
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let config = &last_json(&r)["configs"][0];
    assert_eq!(config["written"], false);
    assert!(config["why"].as_str().unwrap().contains("CARGO_REGISTRIES_LINKED_INDEX"), "{config}");
    assert!(!repo.work().join("services/app/.cargo/config.toml").exists());
}

#[test]
fn an_alias_not_in_libraries_json_is_refused_and_no_manifest_is_touched() {
    // Given a consumer and no registry called nowhere
    let repo = with_app("");
    let before = repo.read("services/app/Cargo.toml");

    // When the library is retargeted to it
    let r = repo.lib(&["retarget", "fixture-core", "nowhere"]);

    // Then it is refused with the aliases that exist, and nothing changed
    assert_eq!(r.exit_code, 1);
    let out = last_json(&r);
    assert!(out["next"].as_str().unwrap().contains("mla"), "{out}");
    assert_eq!(repo.read("services/app/Cargo.toml"), before);
}
