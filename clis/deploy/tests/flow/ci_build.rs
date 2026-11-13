//! `wf ship build <app> --tag <tag>`: the images of an app are built and pushed with the docker CLI against whatever daemon DOCKER_HOST names.
//! The docker CLI is a stub first on PATH that records each call and what it was given on stdin, as the daemon's front door.
use crate::ship::Ship;
use crate::support::{create_stub, RunResult};

const TOKEN: &str = "dckr_pat_super-secret-registry-credential";
const HASH: &str = "0123456789abcdef01234567";

/// `docker`: logs `argv` per call to docker.log and stdin to docker.stdin (separated by a line of `--`), and fails the `imagetools inspect`
/// of an image listed in $DOCKER_HAS (the registry holds it) and any command containing $DOCKER_FAIL_ON.
fn fake_docker(s: &Ship) {
    create_stub(
        &s.dir.path().join("bin/docker"),
        "echo \"$*\" >> \"$STUB_DIR/docker.log\"\n\
         echo \"DOCKER_HOST=$DOCKER_HOST\" >> \"$STUB_DIR/docker.env\"\n\
         case \"$*\" in *--password-stdin*|*\" -\") { cat; echo --; } >> \"$STUB_DIR/docker.stdin\";; esac\n\
         if [ -n \"$DOCKER_FAIL_ON\" ]; then case \"$*\" in *\"$DOCKER_FAIL_ON\"*) echo \"fake daemon: failed\" >&2; exit 7;; esac; fi\n\
         if [ -n \"$DOCKER_FLAKY_BUILDS\" ]; then case \"$*\" in \"buildx build\"*) n=$(cat \"$STUB_DIR/flaky.count\" 2>/dev/null || echo 0); n=$((n+1)); echo $n > \"$STUB_DIR/flaky.count\"; if [ $n -le $DOCKER_FLAKY_BUILDS ]; then echo \"registry: 504 Gateway Time-out\" >&2; exit 9; fi;; esac; fi\n\
         case \"$*\" in\n\
           \"buildx inspect\"*|\"context inspect\"*) exit 1;;\n\
           \"buildx imagetools inspect\"*) for h in $DOCKER_HAS; do case \"$*\" in *\"$h\"*) exit 0;; esac; done; exit 1;;\n\
         esac",
    );
}

fn calls(s: &Ship) -> Vec<String> {
    s.log("docker.log")
}

fn build(s: &Ship, app: &str, env: &[(&str, &str)]) -> RunResult {
    let mut all = vec![("DOCKERHUB_TOKEN", TOKEN), ("DOCKER_HOST", "tcp://127.0.0.1:2376")];
    all.extend_from_slice(env);
    s.wf(&["ship", "build", app, "--tag", "sha-abc1234", "--ref", "HEAD"], &all)
}

#[test]
fn an_app_and_the_sidecars_its_compose_pulls_are_built_and_pushed_on_the_tag_to_the_daemon_docker_host_names() {
    // Given an app with two sidecar images and a daemon that holds none of them
    let s = Ship::new();
    fake_docker(&s);

    // When it is built
    let r = build(&s, "remote-chat-agent", &[]);

    // Then the login reads the credential from stdin, a builder is made and used, and each image is built and pushed on the tag
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let log = calls(&s);
    assert_eq!(log[0], "login -u nuesslerm --password-stdin");
    assert_eq!(log[1], "buildx inspect ci-builder");
    assert_eq!(log[2], "buildx create --name ci-builder --driver docker-container --bootstrap");
    assert_eq!(log[3], "buildx use ci-builder");
    let pushed: Vec<&String> = log.iter().filter(|c| c.starts_with("buildx build")).collect();
    assert_eq!(pushed.len(), 3);
    for (call, image) in pushed.iter().zip(["remote-chat-agent", "remote-chat-agent-stt", "opencode-server"]) {
        assert!(call.contains(&format!("-t docker.io/nuesslerm/{image}:sha-abc1234 -t docker.io/nuesslerm/{image}:nx-{HASH}")) && call.contains("--push") && call.contains("--platform linux/arm64"), "{call}");
        assert!(!call.contains("--cache-to"), "a build without --cache-push only reads the cache: {call}");
        assert!(call.contains(&format!("--label wf.input-hash={HASH}")) && call.contains(&format!("--cache-from type=registry,ref=docker.io/nuesslerm/{image}:buildcache")), "{call}");
    }

    // and every docker call went to the daemon of DOCKER_HOST, and the verb answers with one JSON line naming the images
    let env = std::fs::read_to_string(s.dir.path().join("docker.env")).unwrap();
    assert!(env.lines().all(|l| l == "DOCKER_HOST=tcp://127.0.0.1:2376"), "{env}");
    let answer: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.stdout));
    assert_eq!((answer["app"].as_str(), answer["tag"].as_str()), (Some("remote-chat-agent"), Some("sha-abc1234")));
    let names: Vec<&str> = answer["images"].as_array().unwrap().iter().map(|i| i["image"].as_str().unwrap()).collect();
    assert_eq!(names, ["remote-chat-agent", "remote-chat-agent-stt", "opencode-server"]);
}

#[test]
fn an_image_of_a_hash_the_registry_holds_is_retagged_with_the_commit_and_the_credential_is_never_printed() {
    // Given the registry holding the image of this input hash
    let s = Ship::new();
    fake_docker(&s);

    // When it is built
    let r = build(&s, "leetcode-backend", &[("DOCKER_HAS", HASH)]);

    // Then a one-line layer names the commit and is pushed on the tag, with nothing rebuilt
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let log = calls(&s);
    let retag = log.iter().find(|c| c.starts_with("buildx build")).unwrap();
    assert!(retag.ends_with("-t docker.io/nuesslerm/leetcode-backend:sha-abc1234 --push -") && !retag.contains("--cache-from"), "{retag}");
    let stdin = std::fs::read_to_string(s.dir.path().join("docker.stdin")).unwrap();
    assert!(stdin.starts_with(&format!("{TOKEN}--\n")), "the login reads the credential on stdin");
    assert!(stdin.contains(&format!("FROM docker.io/nuesslerm/leetcode-backend:nx-{HASH}\nENV GIT_COMMIT=")), "{stdin}");
    assert!(r.stdout.contains("\"reused\": true"), "{}", r.stdout);

    // and the credential is in no argument of any call and in no output line
    assert!(log.iter().all(|c| !c.contains(TOKEN)));
    assert!(!r.stdout.contains(TOKEN) && !r.stderr.contains(TOKEN));
}

#[test]
fn a_failed_push_stops_the_build_without_leaking_the_credential_and_a_bad_command_line_builds_nothing() {
    // Given a daemon that refuses a build, When the app is built, Then it fails with the exit code and builds no later image
    let s = Ship::new();
    fake_docker(&s);
    let r = build(&s, "remote-chat-agent", &[("DOCKER_FAIL_ON", "remote-chat-agent-stt"), ("WF_BUILD_RETRY_DELAY_MS", "1")]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("docker buildx build failed with exit code 7"), "{}", r.stderr);
    assert!(calls(&s).iter().all(|c| !c.contains("opencode-server")));
    assert!(!r.stdout.contains(TOKEN) && !r.stderr.contains(TOKEN));

    // Given no credential, an unknown app or an unsafe tag, Then it fails before any docker call
    for (args, message) in [
        (vec!["ship", "build", "remote-chat-agent", "--tag", "t1"], "DOCKERHUB_TOKEN is not set"),
        (vec!["ship", "build", "nope", "--tag", "t1"], "nope builds no image"),
        (vec!["ship", "build", "reporting-service", "--tag", "a b"], "unsafe tag"),
    ] {
        let s = Ship::new();
        fake_docker(&s);
        let r = s.wf(&args, &[]);
        assert_eq!(r.exit_code, 1, "{args:?}");
        assert!(r.stderr.contains(message), "{args:?}: {}", r.stderr);
        assert!(calls(&s).is_empty(), "{args:?}");
    }
}

#[test]
fn with_cache_push_each_built_image_exports_its_layer_cache_to_the_registry_ref_it_reads() {
    // Given a daemon holding none of the images, When the app is built with --cache-push (a push to dev)
    let s = Ship::new();
    fake_docker(&s);
    let r = s.wf(&["ship", "build", "remote-chat-agent", "--tag", "sha-abc1234", "--ref", "HEAD", "--cache-push"], &[("DOCKERHUB_TOKEN", TOKEN), ("DOCKER_HOST", "tcp://127.0.0.1:2376")]);

    // Then every build reads and writes the same buildcache ref, in max mode, before it pushes
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let built: Vec<String> = calls(&s).into_iter().filter(|c| c.starts_with("buildx build")).collect();
    assert_eq!(built.len(), 3);
    for (call, image) in built.iter().zip(["remote-chat-agent", "remote-chat-agent-stt", "opencode-server"]) {
        let cache = format!("type=registry,ref=docker.io/nuesslerm/{image}:buildcache");
        assert!(call.contains(&format!("--cache-from {cache} --cache-to {cache},mode=max --push")), "{call}");
    }
}

#[test]
fn the_images_flag_names_what_an_app_builds_without_a_credential_or_a_docker_call() {
    // Given no credential and a docker stub, When an app with images is asked
    let s = Ship::new();
    fake_docker(&s);
    let with = s.wf(&["ship", "build", "remote-chat-agent", "--images"], &[]);

    // Then it answers one JSON line with its images, and docker was never called
    let names = |r: &RunResult| serde_json::from_str::<serde_json::Value>(r.stdout.trim()).unwrap()["images"].clone();
    assert_eq!(with.exit_code, 0, "{}", with.stderr);
    assert_eq!(names(&with), serde_json::json!(["remote-chat-agent", "remote-chat-agent-stt", "opencode-server"]));
    assert!(calls(&s).is_empty());
}

fn promote(s: &Ship, args: &[&str], env: &[(&str, &str)]) -> RunResult {
    let mut all = vec![("DOCKERHUB_TOKEN", TOKEN), ("DOCKER_HOST", "tcp://127.0.0.1:2376")];
    all.extend_from_slice(env);
    let mut argv = vec!["ship", "promote"];
    argv.extend_from_slice(args);
    s.wf(&argv, &all)
}

#[test]
fn promoting_an_app_points_latest_of_each_of_its_images_at_the_tag_registry_to_registry_without_printing_the_credential() {
    // Given an app whose images are vm-ops-api-rs and watchdog and a stub docker
    let s = Ship::new();
    fake_docker(&s);

    // When it is promoted on a tag
    let r = promote(&s, &["vm-ops-api", "--tag", "sha-abc12345"], &[]);

    // Then each image's latest is created from the tag with imagetools, after a login on stdin, and nothing is built
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let log = calls(&s);
    assert_eq!(log[0], "login -u nuesslerm --password-stdin");
    let created: Vec<&String> = log.iter().filter(|c| c.starts_with("buildx imagetools create")).collect();
    assert_eq!(
        created,
        [
            "buildx imagetools create -t docker.io/nuesslerm/vm-ops-api-rs:latest docker.io/nuesslerm/vm-ops-api-rs:sha-abc12345",
            "buildx imagetools create -t docker.io/nuesslerm/watchdog:latest docker.io/nuesslerm/watchdog:sha-abc12345",
        ]
    );
    assert!(log.iter().all(|c| !c.starts_with("buildx build")));
    let answer: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.stdout));
    assert_eq!((answer["app"].as_str(), answer["tag"].as_str()), (Some("vm-ops-api"), Some("sha-abc12345")));

    // and the credential is in no argument and in no output line
    assert!(log.iter().all(|c| !c.contains(TOKEN)));
    assert!(!r.stdout.contains(TOKEN) && !r.stderr.contains(TOKEN));
}

#[test]
fn a_promotion_without_a_tag_or_a_credential_or_for_an_app_without_images_calls_docker_never() {
    // Given each bad command line, Then it fails with its message before any docker call
    for (args, env_token, message) in [
        (vec!["vm-ops-api"], true, "ship promote needs --tag <tag>"),
        (vec!["vm-ops-api", "--tag", "a b"], true, "unsafe tag"),
        (vec!["nope", "--tag", "t1"], true, "nope builds no image"),
        (vec!["vm-ops-api", "--tag", "t1"], false, "DOCKERHUB_TOKEN is not set"),
    ] {
        let s = Ship::new();
        fake_docker(&s);
        let r = if env_token { promote(&s, &args, &[]) } else { s.wf(&[&["ship", "promote"][..], &args[..]].concat(), &[]) };
        assert_eq!(r.exit_code, 1, "{args:?}");
        assert!(r.stderr.contains(message), "{args:?}: {}", r.stderr);
        assert!(calls(&s).is_empty(), "{args:?}");
    }
}

#[test]
fn a_failed_promotion_fails_the_verb_and_leaves_the_later_images_alone() {
    // Given a registry that refuses the first image
    let s = Ship::new();
    fake_docker(&s);

    // When the app is promoted
    let r = promote(&s, &["vm-ops-api", "--tag", "t1"], &[("DOCKER_FAIL_ON", "vm-ops-api-rs:latest")]);

    // Then the exit code is 1 and the second image was not touched
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("docker buildx imagetools failed"), "{}", r.stderr);
    assert!(calls(&s).iter().all(|c| !c.contains("watchdog")));
}

#[test]
fn with_no_push_every_image_is_built_cold_and_nothing_is_pushed_tagged_or_cache_exported() {
    // Given a registry that already holds the hash of every image of the app (what the CI would only re-tag)
    let s = Ship::new();
    fake_docker(&s);
    let r = s.wf(
        &["ship", "build", "remote-chat-agent", "--tag", "sha-abc1234", "--ref", "HEAD", "--platform", "linux/amd64", "--no-push", "--cache-push"],
        &[("DOCKERHUB_TOKEN", TOKEN), ("DOCKER_HOST", "tcp://127.0.0.1:2376"), ("DOCKER_HAS", "nx-")],
    );

    // When the images are built with --no-push
    assert_eq!(r.exit_code, 0, "{}", r.stderr);

    // Then each is built from its Dockerfile for the platform asked, to no output, with no push, no cache export and no registry lookup of its hash
    let all = calls(&s);
    let built: Vec<&String> = all.iter().filter(|c| c.starts_with("buildx build")).collect();
    assert_eq!(built.len(), 3, "{all:?}");
    for call in built {
        assert!(call.contains("--platform linux/amd64") && call.contains("--output type=cacheonly"), "{call}");
        assert!(!call.contains("--push") && !call.contains("--cache-to"), "{call}");
    }
    assert!(!all.iter().any(|c| c.contains("imagetools")), "{all:?}");

    // and the report says nothing was pushed, and the credential stays out of the output
    assert!(r.stdout.contains("\"pushed\": false") && !r.stdout.contains("\"pushed\": true"), "{}", r.stdout);
    assert!(!r.stdout.contains(TOKEN) && !r.stderr.contains(TOKEN));
}

#[test]
fn with_fresh_every_image_is_rebuilt_without_the_layer_cache_even_when_the_registry_holds_its_hash_and_is_pushed() {
    // Given a registry that already holds the hash of every image of the app
    let s = Ship::new();
    fake_docker(&s);

    // When the images are built with --fresh
    let r = s.wf(
        &["ship", "build", "remote-chat-agent", "--tag", "sha-abc1234", "--ref", "HEAD", "--fresh"],
        &[("DOCKERHUB_TOKEN", TOKEN), ("DOCKER_HOST", "tcp://127.0.0.1:2376"), ("DOCKER_HAS", "nx-")],
    );

    // Then each image is built from its Dockerfile with no cache and pushed, none is re-tagged from the registry, and the report says so
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let all = calls(&s);
    let built: Vec<&String> = all.iter().filter(|c| c.starts_with("buildx build")).collect();
    assert_eq!(built.len(), 3, "{all:?}");
    for call in built {
        assert!(call.contains("--no-cache") && call.contains("--push") && !call.contains("--cache-from"), "{call}");
    }
    assert!(!all.iter().any(|c| c.contains("imagetools inspect")), "{all:?}");
    assert!(r.stdout.contains("\"reused\": false") && !r.stdout.contains("\"reused\": true"), "{}", r.stdout);
}


#[test]
fn a_build_that_meets_a_registry_504_twice_is_run_again_and_proceeds_on_the_third_attempt() {
    // Given a registry that answers 504 to the first two builds of an image and then succeeds
    let s = Ship::new();
    fake_docker(&s);

    // When the app is built
    let r = build(&s, "leetcode-backend", &[("DOCKER_FLAKY_BUILDS", "2"), ("WF_BUILD_RETRY_DELAY_MS", "1")]);

    // Then the same build ran three times, the verb succeeded and the image is reported pushed
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(calls(&s).iter().filter(|c| c.starts_with("buildx build")).count(), 3);
    assert!(r.stdout.contains("\"pushed\": true"), "{}", r.stdout);
}

#[test]
fn a_build_the_registry_never_answers_is_attempted_four_times_and_then_fails_with_the_registry_error() {
    // Given a registry that always answers 504
    let s = Ship::new();
    fake_docker(&s);

    // When the app is built
    let r = build(&s, "leetcode-backend", &[("DOCKER_FLAKY_BUILDS", "99"), ("WF_BUILD_RETRY_DELAY_MS", "1")]);

    // Then it tried four times, failed, and the registry's 504 is in the output
    assert_eq!(r.exit_code, 1);
    assert_eq!(calls(&s).iter().filter(|c| c.starts_with("buildx build")).count(), 4);
    assert!(r.stderr.contains("504 Gateway Time-out") && r.stderr.contains("docker buildx build failed with exit code 9"), "{}", r.stderr);
}
