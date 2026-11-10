use super::support::*;

fn released_twice() -> Repo {
    let repo = Repo::new(&libraries_json("mla", "", ""));
    for _ in 0..2 {
        assert_eq!(repo.lib(&["release", "fixture-core", "patch"]).exit_code, 0);
    }
    repo
}

#[test]
fn status_names_the_latest_release_and_what_each_service_pins_and_prints_no_link() {
    // Given 0.1.1 and 0.1.2 released, one service pinning 0.1.1 from the registry and one still on the path
    let repo = released_twice();
    repo.write("services/a/Cargo.lock", &lock("fixture-core", "0.1.1", true));
    repo.write("services/b/Cargo.lock", &lock("fixture-core", "0.1.2", false));
    repo.commit("locks");

    // When status runs
    let r = repo.lib(&["status"]);

    // Then it is one JSON object with the latest version, every release, each pin, how far behind the registry pin is
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.stdout.trim().lines().count(), 1);
    let out = last_json(&r);
    let core = &out["libraries"][0];
    assert_eq!((core["name"].as_str(), core["latest"].as_str(), core["released"].clone()), (Some("fixture-core"), Some("0.1.2"), serde_json::json!(["0.1.1", "0.1.2"])));
    let pins: Vec<(&str, &str, &str, serde_json::Value)> = core["pins"].as_array().unwrap().iter().map(|p| (p["service"].as_str().unwrap(), p["version"].as_str().unwrap(), p["source"].as_str().unwrap(), p["behind"].clone())).collect();
    assert_eq!(pins, [("services/a", "0.1.1", "mla", serde_json::json!(1)), ("services/b", "0.1.2", "path", serde_json::Value::Null)]);
    assert!(!r.stdout.contains("SECRET-PAR-TOKEN"));
}

#[test]
fn status_reads_the_index_through_the_proxy_with_no_link_in_the_environment() {
    // Given two releases and no link of any kind in the environment
    let repo = released_twice();
    let mut opts = repo.opts();
    for var in ["LIBS_READ_PAR_URL", "LIBS_WRITE_PAR_URL"] {
        opts.env.remove(var);
    }
    let before = repo.proxy.requests().len();

    // When status runs
    let r = repo.lib_with(&["status"], opts);

    // Then it still names the releases, and it asked the proxy for the crate's index file
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(last_json(&r)["libraries"][0]["released"], serde_json::json!(["0.1.1", "0.1.2"]));
    assert_eq!(repo.proxy.requests()[before..], ["/index/fi/xt/fixture-core"]);
}

#[test]
fn status_falls_back_to_the_read_link_when_the_proxy_does_not_answer_and_never_prints_it() {
    // Given two releases, a proxy that nothing answers on, and a read link in the environment
    let repo = released_twice();
    let dead = repo.read("libraries.json").replace(&repo.proxy.index_url(), "http://127.0.0.1:1/index/");
    repo.write("libraries.json", &dead);

    // When status runs
    let r = repo.lib(&["status"]);

    // Then the releases still come from the bucket's index through the link, and no link is printed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(last_json(&r)["libraries"][0]["released"], serde_json::json!(["0.1.1", "0.1.2"]));
    assert!(!r.stdout.contains("SECRET-PAR-TOKEN") && !r.stderr.contains("SECRET-PAR-TOKEN"));
}

#[test]
fn status_with_an_unreachable_proxy_and_no_read_link_still_lists_the_pins_and_says_why_it_is_not_ok() {
    // Given a service pin, a proxy nothing answers on and no read link
    let repo = Repo::new(&libraries_json("mla", "", "").replace("@INDEX@", "http://127.0.0.1:1/index/"));
    repo.write("services/a/Cargo.lock", &lock("fixture-core", "0.1.1", true));
    repo.commit("lock");
    let mut opts = repo.opts();
    opts.env.remove("LIBS_READ_PAR_URL");

    // When status runs
    let r = repo.lib_with(&["status"], opts);

    // Then it exits 1 with the reason and the pin is still there, with no prompt
    assert_eq!(r.exit_code, 1);
    let out = last_json(&r);
    assert!(out["ok"] == false && out["error"].as_str().is_some_and(|e| !e.is_empty()), "{out}");
    assert_eq!(out["libraries"][0]["pins"][0]["version"], "0.1.1");
}
