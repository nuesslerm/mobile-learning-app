//! `wf ci comment` through the built binary, against a loopback fake of the GitHub API: the jobs of a run, a job log behind the
//! redirect GitHub answers with, and the commit comments. No request leaves the machine.
use crate::support::{last_json, run_wf, MockReply, MockRequest, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// A log cache directory of its own for each wf run, so a test never reads the log another test cached under the same run and job ids.
fn log_cache() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("ci-log-cache-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))).display().to_string()
}

/// The options of a run with the labelling variables of the machine the tests run on (a hosted pipeline sets `WF_CI_LABEL`) cleared: a test that wants one sets it.
fn ci_opts() -> RunOpts {
    RunOpts::default().env("WF_CI_LABEL", "").env("WF_CI_RUNNER_STATS_DIR", "")
}

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const TOKEN: &str = "flowtesttoken-0123456789";
const COMMENTS: &str = "/repos/o/r/commits/0123456789abcdef0123456789abcdef01234567/comments";
const PR_COMMENTS: &str = "/repos/o/r/issues/7/comments";
const PASS_LOG: &str = "2026-10-23T10:00:05.0000000Z > nx run crate-wf-report:cargo-flow  [local cache]
2026-10-23T10:00:06.0000000Z > nx run crate-wf-plan:cargo-flow
2026-10-23T10:00:07.0000000Z verify: nx lint over 2 projects: 1 of 2 tasks replayed from the cache
2026-10-23T10:00:08.0000000Z wf: build cache {\"hits\":9,\"misses\":1,\"puts\":1,\"skipped\":0,\"requests\":12,\"bytes\":100,\"degraded\":null,\"latencyMs\":{\"get\":{\"n\":10,\"sumMs\":630.0,\"maxMs\":210.0},\"put\":{\"n\":1,\"sumMs\":90.0,\"maxMs\":90.0},\"exists\":{\"n\":0,\"sumMs\":0.0,\"maxMs\":0.0}}}
2026-10-23T10:00:09.0000000Z #4 CACHED
2026-10-23T10:00:10.0000000Z #5 DONE 3.1s
2026-10-23T10:00:11.0000000Z {\"exportS\": 219.5, \"uploadS\": 39.3, \"servedS\": 0.2}
";

const FLOW_LOG: &str = "2026-10-23T10:00:01.1234567Z ##[group]Run wf test run --stage flow-tests
2026-10-23T10:00:01.1234567Z echo \"wf: an echoed script line is not a failure\"
2026-10-23T10:00:01.1234567Z ##[endgroup]
2026-10-23T10:01:00.0000000Z \u{1b}[32m   Compiling wf v0.1.0\u{1b}[0m
2026-10-23T10:02:00.0000000Z test ci::keeps_the_table ... ok
2026-10-23T10:02:01.0000000Z test ci::posts_one_comment ... FAILED
2026-10-23T10:02:02.0000000Z thread 'ci::posts_one_comment' panicked at clis/wf/tests/flow/ci.rs:42:9:
2026-10-23T10:02:02.0000000Z assertion `left == right` failed: one comment
2026-10-23T10:02:02.0000000Z   left: 2
2026-10-23T10:02:02.0000000Z  right: 1
2026-10-23T10:02:03.0000000Z using credentials flowtesttoken-0123456789 for the call
2026-10-23T10:02:03.0000000Z leaked ghp_abcdefghijklmnopqrstuvwxyz0123456789 and Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345
2026-10-23T10:02:03.0000000Z fetched https://blob.example/log?sig=SECRETSIG123&se=2026
2026-10-23T10:02:04.0000000Z test result: FAILED. 1 passed; 1 failed; 0 ignored
2026-10-23T10:02:05.0000000Z wf: GET https://objectstorage.eu-frankfurt-1.oraclecloud.com/p/Zm9vYmFyYmF6cXV4Y29yZ2VncmF1bHRnYXJwbHk/n/ns/b/v/o/report.html returned HTTP 403
2026-10-23T10:02:06.0000000Z 2e9df672078b Pulling fs layer
2026-10-23T10:02:06.0000000Z 2e9df672078b Pull complete
2026-10-23T10:02:07.0000000Z Error response from daemon: removal of container 3fb6baf70133 is already in progress
2026-10-23T10:03:03.0000000Z wf: build cache {\"hits\":0,\"misses\":12,\"puts\":0}
2026-10-23T10:03:03.5000000Z wf: sccache {\"hits\":\"0\",\"misses\":\"22\"}
2026-10-23T10:03:04.0000000Z ##[error]Process completed with exit code 101.
2026-10-23T10:03:05.0000000Z Post job cleanup.
";

struct Github {
    server: MockServer,
}

impl Github {
    /// `wf ci <args>` (not only `comment`) with the run's environment.
    fn ci_wf(&self, args: &[&str]) -> RunResult {
        self.ci_wf_with(args, &[])
    }

    /// The same with more variables of the run (`WF_CI_LABEL`).
    fn ci_wf_with(&self, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
        let mut argv = vec!["ci"];
        argv.extend(args);
        let mut opts = ci_opts();
        for (k, v) in extra {
            opts = opts.env(k, v);
        }
        run_wf(
            &argv,
            &opts
                .env("GITHUB_TOKEN", TOKEN)
                .env("GITHUB_REPOSITORY", "o/r")
                .env("GITHUB_RUN_ID", "77")
                .env("GITHUB_SHA", SHA)
                .env("GITHUB_API_URL", &self.url())
                .env("GITHUB_SERVER_URL", "https://github.com")
                .env("GITHUB_REF_NAME", "dev")
                .env("WF_CI_LOG_CACHE", &log_cache())
                .env("NO_PROXY", "127.0.0.1")
                .env("no_proxy", "127.0.0.1"),
        )
    }

    fn start() -> Github {
        Github { server: MockServer::start(None) }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port)
    }

    /// The run's jobs, the logs behind a redirect, the commit's comments, and what creating or updating one answers.
    fn serve(&self, jobs: &Value, comments: Value, post: MockReply, patch: MockReply) {
        self.serve_with(jobs, comments, json!([]), post, patch)
    }

    /// As `serve`, with the comments already on the pull request.
    fn serve_with(&self, jobs: &Value, comments: Value, pr_comments: Value, post: MockReply, patch: MockReply) {
        self.serve_log(jobs, comments, pr_comments, post, patch, FLOW_LOG)
    }

    /// As `serve_with`, with the text of the failed job's log.
    fn serve_log(&self, jobs: &Value, comments: Value, pr_comments: Value, post: MockReply, patch: MockReply, flow_log: &str) {
        let mut table = vec![
            ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1".to_string(), MockReply::json(200, json!([{ "number": 7 }]))),
            (format!("GET {PR_COMMENTS}?per_page=100&page=1"), MockReply::json(200, pr_comments)),
            (format!("POST {PR_COMMENTS}"), MockReply::json(201, json!({ "id": 9100 }))),
            ("PATCH /repos/o/r/issues/comments/9100".to_string(), MockReply::json(200, json!({ "id": 9100 }))),
            ("GET /repos/o/r/actions/jobs/500/logs".to_string(), MockReply::text(200, PASS_LOG)),
            ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1".to_string(), MockReply::json(200, json!({ "total_count": jobs.as_array().unwrap().len(), "jobs": jobs }))),
            (format!("GET {COMMENTS}?per_page=100&page=1"), MockReply::json(200, comments)),
            (format!("POST {COMMENTS}"), post),
            ("PATCH /repos/o/r/comments/9001".to_string(), patch),
        ];
        let mut redirect = MockReply::json(302, json!({}));
        redirect.headers.insert("Location".into(), format!("{}/blob/501", self.url()));
        let mut log = MockReply::text(200, flow_log);
        log.headers.insert("Content-Type".into(), "text/plain".into());
        table.extend([("GET /repos/o/r/actions/jobs/501/logs".to_string(), redirect), ("GET /blob/501".to_string(), log)]);
        self.server.routes(&table.iter().map(|(key, reply)| (key.as_str(), reply.clone())).collect::<Vec<_>>());
    }

    fn sent(&self, method: &str) -> Vec<MockRequest> {
        self.server.requests().into_iter().filter(|r| r.method == method).collect()
    }

    fn comment_wf(&self, args: &[&str]) -> RunResult {
        self.comment_wf_with(args, &[])
    }

    fn comment_wf_with(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut argv = vec!["ci", "comment"];
        argv.extend(args);
        let opts = ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &self.url())
            .env("GITHUB_SERVER_URL", "https://github.com")
            .env("GITHUB_REF_NAME", "dev")
            .env("RUNNER_NAME", "vm-main")
            .env("WF_CI_LOG_CACHE", &log_cache())
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1");
        let opts = env.iter().fold(opts, |opts, (key, value)| opts.env(key, value));
        run_wf(&argv, &opts)
    }
}

fn job(id: u64, name: &str, conclusion: &str, done: &str, steps: Value) -> Value {
    json!({ "id": id, "name": name, "status": "completed", "conclusion": conclusion, "started_at": "2026-10-23T10:00:00Z", "completed_at": done, "html_url": format!("https://github.com/o/r/actions/runs/77/job/{id}"), "steps": steps })
}

/// plan passed, flow-tests failed at its second step, clis and the deploy were skipped after it, and one job had been skipped by the plan before.
fn red_run() -> Value {
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    json!([
        job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])),
        job(501, "flow-tests", "failure", "2026-10-23T10:03:05Z", steps),
        job(502, "clis", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(503, "deploy (vm-ops-api)", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(504, "android-e2e-tests", "skipped", "2026-10-23T10:00:20Z", json!([]))
    ])
}

fn created() -> MockReply {
    MockReply::json(201, json!({ "id": 9001 }))
}

fn updated() -> MockReply {
    MockReply::json(200, json!({ "id": 9001 }))
}

fn posted_body(request: &MockRequest) -> String {
    serde_json::from_str::<Value>(&request.body).unwrap()["body"].as_str().unwrap().to_string()
}

#[test]
fn a_red_run_gets_one_commit_comment_with_a_table_of_what_failed_and_no_secret_in_it() {
    // Given a run whose flow-tests job failed, with its log behind a redirect, and a commit with no comment yet
    let github = Github::start();
    github.serve(&red_run(), json!([]), created(), updated());

    // When the comment is previewed with --dry-run
    let preview = github.comment_wf(&["--dry-run"]);
    // Then it names the failed job and shows the body, and nothing is posted
    let shown = last_json(&preview);
    assert_eq!((preview.exit_code, shown["ok"].clone(), shown["posted"].clone(), shown["dryRun"].clone(), shown["failedJobs"].clone()), (0, json!(true), json!(false), json!(true), json!(["flow-tests"])), "{}{}", preview.stdout, preview.stderr);
    assert!(github.sent("POST").is_empty() && github.sent("PATCH").is_empty());

    // When the comment is posted
    github.server.clear();
    let r = github.comment_wf(&[]);
    // Then exactly one comment is created on the pushed commit, and the line says so
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone(), out["posted"].clone(), out["action"].clone(), out["commentId"].clone(), out["failedJobs"].clone()), (0, json!(true), json!(true), json!("created"), json!(9001), json!(["flow-tests"])), "{}{}", r.stdout, r.stderr);
    let posts: Vec<MockRequest> = github.sent("POST").into_iter().filter(|p| p.url == COMMENTS).collect();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let body = posted_body(&posts[0]);
    let after_marker = |text: &str| text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    assert_eq!(after_marker(shown["body"].as_str().unwrap()), after_marker(&body), "the preview is the body that is posted");

    // And a reader finds the verdict, the commit and the run, and the report: each job's cache hit rate, the Nx replays, the docker
    // layers, the EAS seconds, and for each crate whether its tests ran or were replayed
    assert!(body.starts_with("<!-- ci-comment run:77 runner:vm-main -->\n"), "{body}");
    assert!(body.lines().nth(1).is_some_and(|title| title.contains("failed") && title.contains("0123456")), "{body}");
    for link in ["https://github.com/o/r/commit/0123456789abcdef0123456789abcdef01234567", "https://github.com/o/r/actions/runs/77"] {
        assert!(body.contains(link), "{link} missing: {body}");
    }
    for fact in ["90% hit", "Nx replayed 1 of 2", "docker layers cached 1 of 2", "EAS export 219.5 s, upload 39.3 s, served 0.2 s"] {
        assert!(body.contains(fact), "{fact} missing: {body}");
    }
    let line_of = |needle: &str| body.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} missing: {body}")).to_string();
    assert!(line_of("crate-wf-report").contains("replayed") && line_of("crate-wf-plan").contains(" ran"), "{body}");
    // and the same body goes to the open pull request
    let on_pr: Vec<MockRequest> = github.sent("POST").into_iter().filter(|p| p.url == PR_COMMENTS).collect();
    assert_eq!(on_pr.len(), 1, "{on_pr:?}");
    assert_eq!(after_marker(&posted_body(&on_pr[0])), after_marker(&body), "the pull request gets the report with its own marker, the commit with the run's");
    assert!(body.contains("flow-tests") && body.contains("Flow tests") && body.contains("3m 05s"), "the failed job, its step and how long it took: {body}");
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("test ci::posts_one_comment ... FAILED") && !failure.contains("sccache {"), "the cache figures printed at the end of the log are not the cause: {failure}");
    let first_cause = body.find("test ci::posts_one_comment ... FAILED").expect("the cause");
    assert!(first_cause < body.find("Nx replayed").expect("the report"), "what failed comes before the figures: {body}");
    for cause in ["test ci::posts_one_comment ... FAILED", "panicked at clis/wf/tests/flow/ci.rs:42:9:", "assertion `left == right` failed: one comment", "test result: FAILED. 1 passed; 1 failed; 0 ignored"] {
        assert!(body.contains(cause), "{cause} missing from {body}");
    }
    assert!(body.contains("Error response from daemon: removal of container 3fb6baf70133 is already in progress") && !body.contains("Pull complete"), "a docker daemon error is a cause and pull progress is noise: {body}");
    let skipped = line_of("Skipped because of the failure");
    assert!(skipped.contains("`clis`") && skipped.contains("`deploy (vm-ops-api)`") && !skipped.contains("android-e2e-tests") && !body.contains("an echoed script line"), "{body}");
    // and a collapsed block with more of the log, stripped of its timestamps and colours
    assert!(body.contains("<details><summary>flow-tests: ") && body.contains("Compiling wf v0.1.0") && body.contains("[Open the job](https://github.com/o/r/actions/runs/77/job/501)"), "{body}");
    assert!(!body.contains("2026-10-23T10:01") && !body.contains('\u{1b}'), "{body}");

    // And no token and no pre-authenticated path is in it, in the comment or in the output
    for secret in [TOKEN, "ghp_abcdef", "abcdefghijklmnopqrstuvwxyz012345", "Zm9vYmFyYmF6", "SECRETSIG123"] {
        assert!(!body.contains(secret) && !r.stdout.contains(secret) && !r.stderr.contains(secret), "{secret} leaked: {body}");
    }
    assert!(body.contains("/p/[REDACTED]/n/ns/b/v/o/report.html returned HTTP 403"), "{body}");

    // And the API calls carry the token while the log's redirect target never sees it
    let (calls, bearer) = (github.server.requests(), format!("Bearer {TOKEN}"));
    assert!(calls.iter().filter(|c| !c.url.starts_with("/blob/")).all(|c| c.auth.as_deref() == Some(bearer.as_str())), "{calls:?}");
    assert!(calls.iter().filter(|c| c.url.starts_with("/blob/")).all(|c| c.auth.is_none()) && calls.iter().any(|c| c.url == "/blob/501"), "{calls:?}");
}

/// The first 26 lines of a job log of runner 2.338.2 as GitHub stored it (a UTF-8 byte order mark in front of the first line, the runner
/// header, the token permissions group, the checkout guard), copied from run 41208868377.
const RUNNER_2_338_2_HEAD: &str = include_str!("runner_2_338_2_head.log");

#[test]
fn a_failed_job_of_runner_2_338_2_shows_its_cause_not_the_runner_header() {
    // Given a failed flow-tests job whose log is the real head of a 2.338.2 log (byte order mark included) and then a failing step
    let log = format!(
        "{RUNNER_2_338_2_HEAD}2026-10-31T14:08:30.1000000Z ##[group]Run wf test flow\n\
         2026-10-31T14:08:30.1100000Z wf test flow --all\n\
         2026-10-31T14:08:30.1200000Z ##[endgroup]\n\
         2026-10-31T14:09:10.2000000Z test deploy::ship::a_redeploy_is_sent_once ... FAILED\n\
         2026-10-31T14:09:10.2100000Z thread 'deploy::ship::a_redeploy_is_sent_once' panicked at clis/deploy/tests/flow/ship.rs:88:9:\n\
         2026-10-31T14:09:10.2200000Z assertion `left == right` failed: one redeploy\n\
         2026-10-31T14:09:10.2300000Z ##[error]Process completed with exit code 101.\n"
    );
    assert!(log.starts_with('\u{feff}'), "the fixture keeps the byte order mark GitHub writes");
    let github = Github::start();
    github.serve_log(&red_run(), json!([]), json!([]), created(), updated(), &log);

    // When the comment is previewed
    let preview = github.comment_wf(&["--dry-run"]);

    // Then the failed job is named and the lines that say why are in the comment, ahead of the runner header and the permissions group
    let shown = last_json(&preview);
    assert_eq!((preview.exit_code, shown["failedJobs"].clone()), (0, json!(["flow-tests"])), "{}{}", preview.stdout, preview.stderr);
    let body = shown["body"].as_str().unwrap();
    for cause in ["test deploy::ship::a_redeploy_is_sent_once ... FAILED", "panicked at clis/deploy/tests/flow/ship.rs:88:9:", "assertion `left == right` failed: one redeploy"] {
        assert!(body.contains(cause), "{cause} missing from {body}");
    }
    let failure = body.split("### Jobs").next().unwrap();
    assert!(!failure.contains("Process completed with exit code"), "GitHub's generic exit line says nothing about why: {failure}");
    assert!(!failure.contains("Current runner version") && !failure.contains("GITHUB_TOKEN Permissions"), "the runner header is not the cause: {failure}");
    assert!(!body.contains("2026-10-31T14:09") && !body.contains('\u{feff}'), "timestamps and the byte order mark are stripped: {body}");
}

#[test]
fn a_re_run_of_the_same_run_updates_its_comment_instead_of_posting_a_second() {
    // Given a red run that has posted its comment once
    let github = Github::start();
    github.serve(&red_run(), json!([]), created(), updated());
    assert_eq!(github.comment_wf(&[]).exit_code, 0);
    let first = posted_body(&github.sent("POST")[0]);
    // and the commit now lists that comment beside another one
    github.serve(&red_run(), json!([{ "id": 5, "body": "looks good" }, { "id": 9001, "body": first }]), created(), updated());
    github.server.clear();

    // When the same run fails again and wf comments
    let r = github.comment_wf(&[]);

    // Then the comment that carries the run's marker is updated in place and no second one is posted
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["posted"].clone(), out["action"].clone(), out["commentId"].clone()), (0, json!(true), json!("updated"), json!(9001)), "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").iter().all(|p| p.url == PR_COMMENTS), "only the pull request gets a new comment: it has none of this run's yet");
    let patches = github.sent("PATCH");
    assert_eq!(patches.len(), 1, "{patches:?}");
    assert_eq!(patches[0].url, "/repos/o/r/comments/9001");
    let body = posted_body(&patches[0]);
    assert!(body.starts_with("<!-- ci-comment run:77 runner:vm-main -->") && body.contains("flow-tests") && body.contains("3m 05s"), "{body}");
}

#[test]
fn a_green_run_reports_on_the_pull_request_and_in_the_summary_and_a_refused_comment_never_fails_the_ci() {
    // Given a run whose jobs all passed or were skipped, and a summary file
    let github = Github::start();
    let green = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(502, "clis", "skipped", "2026-10-23T10:00:21Z", json!([]))]);
    github.serve(&green, json!([]), created(), updated());
    let summary = std::env::temp_dir().join(format!("ci-summary-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then no commit comment is made, the pull request gets the report, and the summary carries it
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone(), out["posted"].clone(), out["prComment"].clone(), out["summary"].clone()), (0, json!(true), json!(false), json!({ "number": 7, "action": "created", "commentId": 9100 }), json!(true)), "{}{}", r.stdout, r.stderr);
    assert!(github.sent("POST").iter().all(|p| p.url == PR_COMMENTS));
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.lines().nth(1).is_some_and(|title| title.contains("passed") && title.contains("0123456")) && report.contains("plan") && report.contains("20s"), "{report}");
    assert_eq!(posted_body(&github.sent("POST")[0]).trim_end(), report.trim_end());

    // Given a red run whose comment GitHub refuses
    github.serve(&red_run(), json!([]), MockReply::json(403, json!({ "message": "Resource not accessible by integration" })), updated());
    // When wf comments
    let refused = github.comment_wf(&[]);
    // Then it still exits 0, one stderr line gives the reason, and the line says nothing was posted
    let out = last_json(&refused);
    assert_eq!((refused.exit_code, out["ok"].clone(), out["posted"].clone(), out["failedJobs"].clone()), (0, json!(false), json!(false), json!(["flow-tests"])), "{}{}", refused.stdout, refused.stderr);
    assert!(out["reason"].as_str().unwrap().contains("posting the comment: HTTP 403: Resource not accessible by integration"), "{out}");
    assert_eq!(refused.stderr.lines().count(), 1, "{}", refused.stderr);
    assert!(refused.stderr.starts_with("wf ci comment: posting the comment: HTTP 403"), "{}", refused.stderr);
}

const QUOTA_LOG: &str = "2026-10-23T10:00:08.0000000Z wf: build cache storage quota exceeded: the tenancy's Object Storage is full and no write can succeed
2026-10-23T10:00:09.0000000Z wf: build cache {\"hits\":4,\"misses\":2,\"puts\":0,\"skipped\":0,\"quotaExceeded\":2,\"requests\":8,\"bytes\":0,\"degraded\":null,\"latencyMs\":{\"get\":{\"n\":6,\"sumMs\":60.0,\"maxMs\":20.0},\"put\":{\"n\":2,\"sumMs\":40.0,\"maxMs\":30.0},\"exists\":{\"n\":0,\"sumMs\":0.0,\"maxMs\":0.0}}}
";

#[test]
fn a_green_run_whose_cache_writes_were_refused_for_a_full_tenancy_says_so_in_the_headline_and_on_the_job_and_a_clean_run_does_not() {
    // Given a green run whose flow-tests job logged two cache writes refused with StorageQuotaExceeded, and a green run that logged none
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    let summary = std::env::temp_dir().join(format!("ci-summary-quota-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), QUOTA_LOG);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then the headline flags the full tenancy with the number of refused writes, the job row says it, and the run is not marked failed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.contains("Object Storage full: 2 cache write(s) refused"), "{report}");
    assert!(report.contains("Object Storage quota exceeded: 2 cache write(s) refused"), "{report}");
    assert!(!report.contains("failed"), "{report}");

    // And a run with no refused write has no such flag
    let clean = std::env::temp_dir().join(format!("ci-summary-clean-{}", std::process::id()));
    let _ = std::fs::remove_file(&clean);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), PASS_LOG);
    github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", clean.to_str().unwrap())]);
    assert!(!std::fs::read_to_string(&clean).unwrap().to_lowercase().contains("quota"));
}

#[test]
fn the_pull_request_keeps_only_the_comment_of_the_run_that_reported_last() {
    // Given a pull request that carries the CI comments of two earlier reports and an unrelated comment
    let github = Github::start();
    let older = "<!-- ci-comment run:76 runner:vm-main -->\nolder run";
    let live = "<!-- ci-live run:75 -->\nan earlier live comment";
    github.serve_with(&red_run(), json!([]), json!([{ "id": 3, "body": older }, { "id": 4, "body": live }, { "id": 5, "body": "a human's remark" }]), created(), updated());

    // When this run reports
    let r = github.comment_wf(&[]);

    // Then its own comment is created at the bottom, the two CI comments of the earlier runs are deleted, and the remark is left alone
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["prComment"]["action"].clone()), (0, json!("created")), "{}{}", r.stdout, r.stderr);
    let mut deleted: Vec<String> = github.sent("DELETE").into_iter().map(|d| d.url).collect();
    deleted.sort();
    assert_eq!(deleted, ["/repos/o/r/issues/comments/3", "/repos/o/r/issues/comments/4"]);
}

/// An RFC 3339 instant `secs` seconds from the epoch, as the Actions API writes it.
fn iso(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let (doy, y) = (doe - (365 * yoe + yoe / 4 - yoe / 100), yoe + era * 400);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A green run that finished a few minutes ago, with the VM's numbers served for that span (a gap of `hole_min` minutes in the middle).
fn vm_run(github: &Github, hole_min: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let (start, end) = (now - 600, now - 120);
    let mut recent = job(500, "plan", "success", &iso(end), json!([]));
    recent["started_at"] = json!(iso(start));
    github.serve(&json!([recent]), json!([]), created(), updated());
    let mut points = Vec::new();
    for minute in 0..8i64 {
        if hole_min > 0 && minute > 2 && minute <= 2 + hole_min {
            continue;
        }
        let ts = (start + minute * 60 + 30) * 1000;
        let host = |metric: &str, value: f64| json!({ "ts": ts, "scope": "host", "name": "host", "metric": metric, "value": value });
        points.extend([host("cpu", 20.0 + minute as f64), host("memory_used", 5.0e9), host("memory_total", 12.5e9), host("psi_io", if minute == 4 { 62.0 } else { 3.0 })]);
    }
    let vm = MockServer::start(None);
    vm.routes(&[("GET /api/metrics/series?window=24h&scope=host", MockReply::json(200, json!({ "window": "24h", "points": points })))]);
    let url = format!("http://127.0.0.1:{}", vm.port);
    std::mem::forget(vm);
    url
}

#[test]
fn the_report_shows_what_the_vm_was_doing_while_the_jobs_ran_and_when_it_stopped_sampling() {
    // Given a run that finished minutes ago and a reporting-service that stored the VM's numbers for it
    let github = Github::start();
    let vm = vm_run(&github, 0);

    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &vm), ("WF_TAILNET_SOCKS", "")]);

    // Then the worst I/O wait of the run is in the report with its value, and the VM is said to have kept up
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.contains("62%"), "the I/O wait peak: {body}");
    assert!(body.to_lowercase().contains("no gaps"), "{body}");
    let charts: Vec<&str> = body.split("```mermaid\n").skip(1).map(|c| c.split("```").next().unwrap_or("")).collect();
    assert!(charts.iter().all(|c| c.starts_with("xychart-beta") && c.contains("bar [") && !c.contains("line [")), "only vertical bar charts, which a phone in portrait draws whole: {body}");
    assert!(!body.contains("gantt"), "{body}");
    assert!(charts.iter().any(|c| c.contains("62.0")), "a chart of the I/O wait carries its peak: {body}");

    // Given the VM stopped sampling for four minutes in the middle of the run
    let stalled = vm_run(&github, 4);
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &stalled), ("WF_TAILNET_SOCKS", "")]);
    // Then the report says it stalled
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.to_lowercase().contains("stalled") && !body.to_lowercase().contains("no gaps"), "{body}");
}

#[test]
fn a_report_is_still_made_when_the_vm_cannot_be_asked_and_it_says_so() {
    // Given a run that finished minutes ago and nothing answering where the VM's numbers live
    let github = Github::start();
    let _ = vm_run(&github, 0);
    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", "http://127.0.0.1:9"), ("WF_TAILNET_SOCKS", "")]);
    // Then the report is made, with a line that the VM's stats are unavailable
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (0, json!(true)), "{}{}", r.stdout, r.stderr);
    assert!(out["body"].as_str().unwrap().contains("unavailable"), "{out}");
}

#[test]
fn a_test_run_never_writes_into_the_real_job_summary() {
    // Given a runner whose summary file the test process inherits
    let summary = std::env::temp_dir().join(format!("real-summary-{}", std::process::id()));
    std::fs::write(&summary, "the real summary\n").unwrap();
    let github = Github::start();
    github.serve(&red_run(), json!([]), created(), updated());
    // When wf runs under the test harness with that variable present in the parent environment
    std::env::set_var("GITHUB_STEP_SUMMARY", &summary);
    let r = github.comment_wf(&[]);
    std::env::remove_var("GITHUB_STEP_SUMMARY");
    // Then the fixture's report never reached it
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(std::fs::read_to_string(&summary).unwrap(), "the real summary\n");
}

#[test]
fn jobs_that_failed_the_same_way_are_one_block_naming_each_of_them_and_the_cause_once() {
    // Given two jobs that failed in the same step with the same log
    let github = Github::start();
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    let twins = json!([job(501, "deploy (a)", "failure", "2026-10-23T10:03:05Z", steps.clone()), job(501, "deploy (b)", "failure", "2026-10-23T10:03:05Z", steps), job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([]))]);
    github.serve(&twins, json!([]), created(), updated());

    // When wf reports
    let r = github.comment_wf(&["--dry-run"]);

    // Then both jobs are named in what failed, and the failing test line is in it once
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("deploy (a)") && failure.contains("deploy (b)"), "{failure}");
    assert_eq!(failure.matches("test ci::posts_one_comment ... FAILED").count(), 1, "{failure}");
}

const NX_LOG: &str = "2026-10-23T10:02:00.0000000Z crate-wf-login: error: process didn't exit successfully: `sccache rustc -vV` (exit status: 2)
2026-10-23T10:02:00.1000000Z crate-wf-login: --- stderr
2026-10-23T10:02:00.2000000Z crate-wf-login: sccache: error: Connection to server timed out: Connection refused
2026-10-23T10:02:05.0000000Z {\"ok\":false,\"stage\":\"flow-tests\",\"fresh\":false}
2026-10-23T10:02:06.0000000Z wf: build cache {\"hits\":0,\"misses\":12}
2026-10-23T10:02:06.5000000Z wf: flow-tests: unit in crate-a, crate-b
";

#[test]
fn a_failure_a_task_prints_behind_nxs_project_label_is_the_cause_and_the_stage_summary_is_not() {
    // Given a failed job whose log has the error behind Nx's `crate-x: ` label, then the run's summary lines
    let github = Github::start();
    github.serve_log(&red_run(), json!([]), json!([]), created(), updated(), NX_LOG);

    // When wf reports
    let r = github.comment_wf(&["--dry-run"]);

    // Then the cause is named, and the summary line is not shown as one
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    let failure = body.split("### Jobs").next().unwrap();
    assert!(failure.contains("Connection to server timed out"), "{failure}");
    assert!(!failure.contains("\"stage\"") && !failure.contains("build cache {") && !failure.contains("unit in crate-a"), "{failure}");
}

const TIMED_LOG: &str = "2026-10-23T10:00:00.0000000Z crate-slow: test ci::very_slow ... 
2026-10-23T10:03:20.0000000Z crate-slow: ok
2026-10-23T10:03:21.0000000Z crate-slow: test result: ok. 1 passed; 0 failed; finished in 200.00s
2026-10-23T10:03:30.0000000Z Task Execution Timings:
2026-10-23T10:03:30.0000000Z {
2026-10-23T10:03:30.0000000Z   \"crate-slow:cargo-flow\": 200000
2026-10-23T10:03:30.0000000Z }
2026-10-23T10:03:30.0000000Z   Run duration:      3m 30s
2026-10-23T10:03:30.0000000Z   Cache:             0/1 hit (0%)
2026-10-23T10:03:30.0000000Z   Critical path:     3m 20s (1 task)
2026-10-23T10:03:30.0000000Z   Recoverable time:  10.0s (5% of the run)
2026-10-23T10:03:31.0000000Z wf: sccache {\"hits\":\"3\",\"misses\":\"7\",\"writeErrors\":\"0\"}
2026-10-23T10:03:31.0000000Z wf: nx cost {\"target\":\"cargo-flow\",\"cpuSeconds\":120.0,\"writtenBytes\":2000000000,\"seconds\":100.0}
";

#[test]
fn the_report_says_where_the_time_went_what_waited_who_wrote_the_disk_and_what_the_caches_missed() {
    // Given a run whose second job waited 100 s for the runner and ran a 200 s test, and a VM that stored a container's disk writes
    let github = Github::start();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let start = now - 900;
    let mut plan = job(500, "plan", "success", &iso(start + 100), json!([]));
    (plan["created_at"], plan["started_at"]) = (json!(iso(start)), json!(iso(start)));
    let steps = json!([{ "name": "Flow tests", "conclusion": "success", "started_at": iso(start + 200), "completed_at": iso(start + 600) }]);
    let mut flow = job(501, "flow-tests", "success", &iso(start + 650), steps);
    (flow["created_at"], flow["started_at"]) = (json!(iso(start + 100)), json!(iso(start + 200)));
    github.serve_log(&json!([plan, flow]), json!([]), json!([]), created(), updated(), TIMED_LOG);
    let container = |ts: i64, metric: &str, value: f64| json!({ "ts": ts * 1000, "scope": "container", "name": "agent-shell", "metric": metric, "value": value });
    let containers = json!([container(start + 210, "disk_write_bytes", 1.0e9), container(start + 500, "disk_write_bytes", 3.5e9), container(start + 300, "cpu", 80.0)]);
    let host = |m: i64, metric: &str, value: f64| json!({ "ts": (start + m * 60 + 30) * 1000, "scope": "host", "name": "host", "metric": metric, "value": value });
    let hosts: Vec<Value> = (0..10).flat_map(|m| [host(m, "cpu", 90.0), host(m, "cpu_steal", 10.0)]).collect();
    let vm = MockServer::start(None);
    let since = (start - 60) * 1000;
    vm.routes(&[
        ("GET /api/metrics/series?window=24h&scope=host", MockReply::json(200, json!({ "points": hosts }))),
        (&format!("GET /api/metrics/series?window=24h&scope=container&metric=cpu,memory,disk_read_bytes,disk_write_bytes&since={since}"), MockReply::json(200, json!({ "points": containers }))),
    ]);
    let url = format!("http://127.0.0.1:{}", vm.port);

    // When wf reports
    let r = github.comment_wf_with(&["--dry-run"], &[("WF_VM_STATS_URL", &url), ("WF_TAILNET_SOCKS", "")]);

    // Then the wait is named, the slow test and task are named, the disk writer is named with its bytes, and the cache misses are listed
    let body = last_json(&r)["body"].as_str().unwrap().to_string();
    assert!(body.contains("Waited for the runner: **flow-tests** 1m 40s"), "{body}");
    assert!(body.contains("ci::very_slow") && body.contains("crate-slow:cargo-flow"), "{body}");
    assert!(body.contains("agent-shell") && body.contains("2.5 GB"), "the container that wrote 2.5 GB: {body}");
    assert!(body.contains("sccache 30% (3/10)") && body.contains("Nx runs"), "{body}");
    assert!(body.contains("| flow-tests | cargo-flow | 120 s | 60% | 2.0 GB | 100 s |"), "the stage's CPU seconds, share of both cores and bytes written: {body}");
    assert!(body.contains("the host was busy 1.80 = containers 0.80 + the hypervisor 0.20 + not in any container 0.80 (44% of the busy time)"), "the two-core split of the busy time: {body}");
}

const BASE: i64 = 1_793_000_000;

fn timed_job(id: u64, name: &str, conclusion: &str, secs: i64) -> Value {
    let mut j = job(id, name, conclusion, &iso(BASE + secs), json!([]));
    (j["created_at"], j["started_at"]) = (json!(iso(BASE)), json!(iso(BASE)));
    j
}

fn ledger_line(tasks: &[(&str, &str, &str, i64)]) -> String {
    let list: Vec<Value> = tasks.iter().map(|(task, hash, outcome, ms)| json!({ "task": task, "hash": hash, "outcome": outcome, "ms": ms, "passed": true })).collect();
    format!("2026-10-30T10:00:00.0000000Z wf: nx ledger {}\n", json!({ "tasks": list }))
}

/// This run (77) with one job whose log is `log`, and the other runs of the workflow as `(started, updated)` seconds from BASE.
fn audit_github(current: &Value, log: &str, others: &[(i64, i64)]) -> Github {
    audit_github_of(std::slice::from_ref(current), log, others)
}

/// The same with several jobs; job 500's log is `log`.
fn audit_github_of(jobs: &[Value], log: &str, others: &[(i64, i64)]) -> Github {
    let github = Github::start();
    let runs: Vec<Value> = others.iter().enumerate().map(|(i, (a, b))| json!({ "id": 900 + i, "run_started_at": iso(BASE + a), "updated_at": iso(BASE + b) })).collect();
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": runs }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": jobs.len(), "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, log)),
    ]);
    github
}

struct Audit {
    ledger: std::path::PathBuf,
    nx: std::path::PathBuf,
    limits: std::path::PathBuf,
}

impl Audit {
    /// A ledger file holding `earlier` (as JSON lines), an `nx` that says `affected` changed, and the thresholds file `limits`.
    fn new(name: &str, earlier: &[Value], affected: &str, limits: Value) -> Audit {
        let dir = std::env::temp_dir().join(format!("ci-audit-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (ledger, nx, limits_file) = (dir.join("ledger.jsonl"), dir.join("nx"), dir.join("limits.json"));
        std::fs::write(&ledger, earlier.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        std::fs::write(&nx, format!("#!/bin/sh\nprintf '%s' '{affected}'\n")).unwrap();
        std::process::Command::new("chmod").args(["755", nx.to_str().unwrap()]).status().unwrap();
        std::fs::write(&limits_file, limits.to_string()).unwrap();
        Audit { ledger, nx, limits: limits_file }
    }

    /// Audits the one run 77 (`--run`), or with `extra` whatever else the test passes.
    fn run_with(&self, github: &Github, extra: &[&str]) -> RunResult {
        let mut argv = vec!["ci", "audit"];
        argv.extend_from_slice(extra);
        run_wf(
            &argv,
            &ci_opts()
                .env("GITHUB_TOKEN", TOKEN)
                .env("GITHUB_REPOSITORY", "o/r")
                .env("GITHUB_RUN_ID", "1")
                .env("GITHUB_SHA", SHA)
                .env("GITHUB_API_URL", &github.url())
                .env("WF_CI_LOG_CACHE", &log_cache())
                .env("WF_CI_LEDGER_FILE", self.ledger.to_str().unwrap())
                .env("WF_NX_BIN", self.nx.to_str().unwrap())
                .env("WF_CI_AUDIT_FILE", self.limits.to_str().unwrap())
                .env("WF_VM_STATS_URL", "http://127.0.0.1:9")
                .env("WF_TAILNET_SOCKS", "")
                .env("NO_PROXY", "127.0.0.1")
                .env("no_proxy", "127.0.0.1"),
        )
    }

    fn run(&self, github: &Github, dry: bool) -> RunResult {
        let mut extra = vec!["--run", "77"];
        if dry {
            extra.push("--dry-run");
        }
        self.run_with(github, &extra)
    }
}

fn earlier_task(run: &str, ts: i64, task: &str, hash: &str, outcome: &str, ms: i64) -> Value {
    json!({ "t": "task", "run": run, "sha": format!("sha{run}"), "ts": ts, "job": "flow-tests", "task": task, "hash": hash, "outcome": outcome, "ms": ms, "passed": true })
}

fn kinds(out: &Value) -> Vec<String> {
    out["findings"].as_array().unwrap().iter().map(|b| b["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_task_that_ran_on_inputs_an_earlier_run_already_cached_is_named_with_its_seconds_and_the_next_step_and_fails_nothing() {
    // Given a task that was replayed from the cache before and ran again now under the same input hash
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "ran", 90_000), earlier_task("71", now - 5000, "app:lint", "h1", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h1", "ran", 95_000)]), &[]);
    let audit = Audit::new("hit", &earlier, "", json!({}));

    // When the run is audited
    let r = audit.run(&github, true);

    // Then the cache's miss is a correctness finding with a next step, and the command still exits 0
    let out = last_json(&r);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&out), ["should-have-hit"], "{out}");
    let finding = &out["findings"][0];
    let message = finding["message"].as_str().unwrap();
    assert!(message.contains("app:lint") && message.contains("1m 35s"), "{message}");
    assert_eq!((finding["hard"].clone(), finding["run"].clone(), out["ok"].clone()), (json!(true), json!("77"), json!(false)), "{out}");
    assert!(!finding["next"].as_str().unwrap().is_empty(), "{out}");
}

#[test]
fn a_task_that_ran_because_its_inputs_changed_is_not_waste() {
    // Given a task whose hash is new (its inputs changed), with the project reported as changed
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h2", "ran", 95_000)]), &[]);
    let audit = Audit::new("changed", &earlier, "app\n", json!({}));
    // When the run is audited
    let r = audit.run(&github, true);
    // Then nothing is found
    let out = last_json(&r);
    assert_eq!((r.exit_code, kinds(&out), out["ok"].clone()), (0, Vec::<String>::new(), json!(true)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_hash_that_moved_although_nothing_it_depends_on_changed_is_named_as_drift() {
    // Given a task whose hash differs from the previous run's while nx says no project changed
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "old", "hit", 50)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "new", "hit", 40)]), &[]);
    // When the run is audited
    let r = Audit::new("drift", &earlier, "", json!({})).run(&github, true);
    // Then the drift is named and the command passes
    assert_eq!((r.exit_code, kinds(&last_json(&r))), (0, vec!["key-drift".to_string()]), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_same_work_taking_far_longer_than_before_is_reported_as_the_machine_being_slow() {
    // Given a task that ran twice before under this hash in about ten seconds and now took a minute
    let now = BASE * 1000;
    let earlier = [earlier_task("68", now - 30_000, "app:t", "h", "ran", 10_000), earlier_task("69", now - 20_000, "app:t", "h", "ran", 11_000)];
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:t", "h", "ran", 60_000)]), &[]);
    // When the run is audited
    let r = Audit::new("slow", &earlier, "app\n", json!({})).run(&github, true);
    // Then the slowdown under the same inputs is named
    assert!(kinds(&last_json(&r)).contains(&"slower-same-work".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_wait_behind_another_run_is_not_a_finding_and_an_unexplained_one_is() {
    // Given a job created at BASE that started 15 minutes later
    let mut waited = timed_job(500, "flow-tests", "success", 1500);
    waited["started_at"] = json!(iso(BASE + 900));
    // When another run was holding the runner the whole time
    let busy = audit_github(&waited, "ok\n", &[(0, 950)]);
    let r = Audit::new("wait-a", &[], "", json!({})).run(&busy, true);
    // Then the wait is not reported
    assert!(!kinds(&last_json(&r)).contains(&"runner-wait".to_string()), "{}{}", r.stdout, r.stderr);
    // When no other run was running
    let idle = audit_github(&waited, "ok\n", &[]);
    let r = Audit::new("wait-b", &[], "", json!({})).run(&idle, true);
    // Then the wait is reported
    assert!(kinds(&last_json(&r)).contains(&"runner-wait".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_job_whose_cache_refused_to_write_is_a_correctness_finding_that_still_fails_nothing() {
    // Given a run whose job log says the build cache read only
    let log = "wf: build cache reads only for this run, dirty: the work tree has changes: ARCHITECTURE.md\n";
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), log, &[]);
    // When the run is audited
    let r = Audit::new("refused", &[], "", json!({})).run(&github, true);
    // Then the cache is named as a hard finding and the exit code is 0
    let out = last_json(&r);
    assert_eq!((r.exit_code, kinds(&out), out["findings"][0]["hard"].clone()), (0, vec!["cache-read-only".to_string()], json!(true)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_run_with_a_failed_job_is_not_judged_for_time_because_its_timings_say_nothing() {
    let github = audit_github(&timed_job(500, "flow-tests", "failure", 1200), "ok\n", &[]);
    let r = Audit::new("failed", &[], "", json!({})).run(&github, true);
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["runs"][0]["judged"].clone()), (0, json!(false)), "{}{}", r.stdout, r.stderr);
}

#[test]
fn what_a_run_did_is_in_the_ledger_for_the_next_run_to_be_judged_against() {
    // Given a run whose task ran under a new hash
    let github = audit_github(&timed_job(500, "flow-tests", "success", 600), &ledger_line(&[("app:lint", "h9", "ran", 5000)]), &[]);
    let audit = Audit::new("record", &[], "app\n", json!({}));
    // When the audit runs for real
    let first = audit.run(&github, false);
    assert_eq!(first.exit_code, 0, "{}{}", first.stdout, first.stderr);
    // Then a later run that reruns the same task on the same hash after a replay is judged against it
    let stored = std::fs::read_to_string(&audit.ledger).unwrap();
    assert!(stored.contains("\"hash\":\"h9\"") && stored.contains("\"job\":\"flow-tests\""), "{stored}");
}

#[test]
fn the_cpu_seconds_threshold_is_what_names_a_run_and_a_pnpm_pack_outside_flow_tests_is_named() {
    // Given a run whose test job packed the pnpm store and whose window the VM was busy in (the reporting-service is unreachable here, so the
    // CPU-seconds cannot be measured: the threshold only decides when there is a number)
    let pack = "2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=75.2\n";
    let github = audit_github(&timed_job(500, "typecheck-lint-unit-tests", "success", 600), pack, &[]);
    // When the run is audited
    let r = Audit::new("pnpm", &[], "", json!({ "cpuSecondsThreshold": 1 })).run(&github, true);
    // Then the pack is named with the job that did it, and only flow-tests may do it
    let out = last_json(&r);
    let found: Vec<&Value> = out["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "pnpm-packed-outside-flow-tests").collect();
    assert_eq!(found.len(), 1, "{out}");
    assert_eq!(found[0]["subject"], "typecheck-lint-unit-tests", "{out}");
    let own = audit_github(&timed_job(500, "flow-tests", "success", 600), pack, &[]);
    let r = Audit::new("pnpm-own", &[], "", json!({})).run(&own, true);
    assert!(!kinds(&last_json(&r)).contains(&"pnpm-packed-outside-flow-tests".to_string()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_audit_without_a_run_judges_every_completed_run_on_dev_since_the_given_time_oldest_first() {
    // Given three completed runs, one of them older than the window, and a task that reran in the newer run what the older one already cached
    let now = BASE * 1000;
    let earlier = [earlier_task("70", now - 10_000, "app:lint", "h1", "ran", 90_000), earlier_task("71", now - 5000, "app:lint", "h1", "hit", 50)];
    let github = Github::start();
    let run = |id: u64, started: i64, event: &str| json!({ "id": id, "head_sha": SHA, "event": event, "run_started_at": iso(BASE + started) });
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [run(78, 5000, "push"), run(77, 100, "push"), run(60, -90_000, "push"), run(61, 200, "pull_request")] }))),
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": [] }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(500, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, &ledger_line(&[("app:lint", "h1", "ran", 95_000)]))),
        ("GET /repos/o/r/actions/runs/78/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(501, "flow-tests", "failure", 600)] }))),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, "ok\n")),
    ]);
    // When the audit runs with a window start between the old run and the two newer ones
    let r = Audit::new("window", &earlier, "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then the two push runs are judged oldest first (the old run and the pull request's are not), and the miss is named on the first
    let out = last_json(&r);
    let runs: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["run"].as_str().unwrap()).collect();
    assert_eq!((r.exit_code, runs), (0, vec!["77", "78"]), "{}{}", r.stdout, r.stderr);
    assert_eq!(kinds(&out), ["should-have-hit"], "{out}");
    assert_eq!(out["runs"][1]["judged"], json!(false), "{out}");
}

#[test]
fn the_audit_judges_the_hosted_workflow_too_and_calls_a_scheduled_or_dispatched_run_cold() {
    // Given a push run of the VM's workflow, and a scheduled and a pushed run of the GitHub-hosted workflow
    let earlier: [Value; 0] = [];
    let github = Github::start();
    let run = |id: u64, started: i64, event: &str| json!({ "id": id, "head_sha": SHA, "event": event, "run_started_at": iso(BASE + started) });
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [run(77, 100, "push")] }))),
        ("GET /repos/o/r/actions/workflows/ci-hosted.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [run(78, 200, "schedule"), run(79, 300, "push")] }))),
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": [] }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(500, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
        ("GET /repos/o/r/actions/runs/78/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(501, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, &ledger_line(&[("app:lint", "h1", "ran", 95_000)]))),
        ("GET /repos/o/r/actions/runs/79/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(502, "flow-tests", "failure", 600)] }))),
        ("GET /repos/o/r/actions/jobs/502/logs", MockReply::text(200, "ok\n")),
    ]);
    // When the audit runs over a window that holds all three
    let r = Audit::new("hosted", &earlier, "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then all three are listed oldest first, only the scheduled one is cold, the hosted ones say so, and a note says the hosted run's times were not judged
    let out = last_json(&r);
    let seen: Vec<(String, bool, bool)> = out["runs"].as_array().unwrap().iter().map(|r| (r["run"].as_str().unwrap().to_string(), r["cold"] == true, r["hosted"] == true)).collect();
    assert_eq!((r.exit_code, seen), (0, vec![("77".to_string(), false, false), ("78".to_string(), true, true), ("79".to_string(), false, true)]), "{}{}", r.stdout, r.stderr);
    assert!(out["notes"].to_string().contains("GitHub-hosted"), "{out}");
}

#[test]
fn an_audit_still_judges_the_vm_runs_when_the_hosted_workflow_cannot_be_listed_and_says_so() {
    // Given a VM run and a GitHub that answers 404 for the hosted workflow
    let github = Github::start();
    github.server.routes(&[
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(200, json!({ "workflow_runs": [{ "id": 77, "head_sha": SHA, "event": "push", "run_started_at": iso(BASE + 100) }] }))),
        ("GET /repos/o/r/actions/workflows/ci-hosted.yml/runs?branch=dev&status=completed&per_page=50", MockReply::json(404, json!({ "message": "Not Found" }))),
        ("GET /repos/o/r/actions/workflows/deploy.yml/runs?branch=dev&per_page=30", MockReply::json(200, json!({ "workflow_runs": [] }))),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": 1, "jobs": [timed_job(500, "flow-tests", "success", 600)] }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
    ]);
    // When the audit runs
    let r = Audit::new("no-hosted", &[], "", json!({})).run_with(&github, &["--dry-run", "--since", &(BASE - 1000).to_string()]);
    // Then the VM run is judged, the exit code is 0 and a note names the workflow that could not be listed
    let out = last_json(&r);
    let notes = out["notes"].to_string();
    assert_eq!((r.exit_code, out["runs"].as_array().unwrap().len()), (0, 1), "{}{}", r.stdout, r.stderr);
    assert!(notes.contains("ci-hosted.yml"), "{notes}");
}

#[test]
fn preflight_names_every_check_that_failed_and_what_to_do_about_it() {
    // Given a GitHub that refuses the token, a service that is down and an environment with no cache links
    let github = Github::start();
    github.server.routes(&[("GET /rate_limit", MockReply::json(401, json!({ "message": "Bad credentials" })))]);
    let r = run_wf(
        &["ci", "preflight", "--skip", "workspace,toolchain,secrets,runner,ledger"],
        &ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &github.url())
            .env("BUILD_CACHE_READ_PAR_URL", "")
            .env("BUILD_CACHE_WRITE_PAR_URL", "")
            .env("WF_PREFLIGHT_HEALTH", "svc=http://127.0.0.1:9/health")
            .env("WF_TAILNET_SOCKS", "")
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1"),
    );
    // When wf runs the preflight
    let out = last_json(&r);
    // Then it fails, naming the checks that failed, each with what to do
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let failed: Vec<&str> = out["failed"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(failed.contains(&"github") && failed.contains(&"svc") && failed.contains(&"build-cache"), "{out}");
    assert!(out["checks"].as_array().unwrap().iter().filter(|c| c["ok"] == false).all(|c| !c["next"].as_str().unwrap().is_empty()), "{out}");
}

#[test]
fn preflight_warns_when_the_pinned_runner_is_behind_github_and_stays_quiet_when_it_is_not() {
    // Given the runner version compose pins and a GitHub whose latest runner release is a minor ahead, then the same one
    let pinned = std::fs::read_to_string(wf_kit::root::repo_root().join("infra/oracle-cloud/ci-runner/docker-compose.prod.yml")).unwrap();
    let tag = pinned.split("github-runner:").nth(1).unwrap().split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap().to_string();
    let parts: Vec<u64> = tag.split('.').map(|p| p.parse().unwrap()).collect();
    let preflight = |latest: String| {
        let github = Github::start();
        github.server.routes(&[("GET /rate_limit", MockReply::json(200, json!({ "resources": { "core": { "remaining": 4000 } } }))), ("GET /repos/actions/runner/releases/latest", MockReply::json(200, json!({ "tag_name": format!("v{latest}") })))]);
        let r = run_wf(
            &["ci", "preflight", "--skip", "workspace,toolchain,secrets,runner,ledger,build-cache,services"],
            &ci_opts().env("GITHUB_TOKEN", TOKEN).env("GITHUB_REPOSITORY", "o/r").env("GITHUB_RUN_ID", "77").env("GITHUB_SHA", SHA).env("GITHUB_API_URL", &github.url()).env("WF_TAILNET_SOCKS", "").env("NO_PROXY", "127.0.0.1").env("no_proxy", "127.0.0.1"),
        );
        let out = last_json(&r);
        let check = out["checks"].as_array().unwrap().iter().find(|c| c["check"] == "runner-version").cloned().unwrap();
        (r.exit_code, check)
    };

    // When wf runs the preflight against a newer minor
    let (code, behind) = preflight(format!("{}.{}.0", parts[0], parts[1] + 1));
    // Then the check warns, names both versions and what to do, and does not stop the run
    assert_eq!((code, behind["ok"].clone(), behind["required"].clone()), (0, json!(false), json!(false)), "{behind}");
    assert!(behind["detail"].as_str().unwrap().contains(&tag) && behind["next"].as_str().unwrap().contains("redeploy ci-runner"), "{behind}");

    // And against the pinned version itself it passes
    let (_, current) = preflight(tag.clone());
    assert_eq!(current["ok"], json!(true), "{current}");
}

#[test]
fn preflight_passes_when_what_it_checks_is_live() {
    // Given a GitHub that accepts the token and a service that is up
    let github = Github::start();
    github.server.routes(&[("GET /rate_limit", MockReply::json(200, json!({ "resources": { "core": { "remaining": 4000 } } })))]);
    let service = MockServer::start(None);
    service.routes(&[("GET /health", MockReply::json(200, json!({ "ok": true })))]);
    let r = run_wf(
        &["ci", "preflight", "--skip", "workspace,toolchain,secrets,runner,ledger,build-cache"],
        &ci_opts()
            .env("GITHUB_TOKEN", TOKEN)
            .env("GITHUB_REPOSITORY", "o/r")
            .env("GITHUB_RUN_ID", "77")
            .env("GITHUB_SHA", SHA)
            .env("GITHUB_API_URL", &github.url())
            .env("WF_PREFLIGHT_HEALTH", &format!("svc=http://127.0.0.1:{}/health", service.port))
            .env("WF_TAILNET_SOCKS", "")
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1"),
    );
    // Then it passes
    assert_eq!((r.exit_code, last_json(&r)["ok"].clone()), (0, json!(true)), "{}{}", r.stdout, r.stderr);
}

fn live_jobs() -> Value {
    let mut preflight = timed_job(500, "preflight", "success", 12);
    preflight["status"] = json!("completed");
    let running = json!({ "id": 501, "name": "flow-tests", "status": "in_progress", "conclusion": null, "started_at": iso(BASE + 20), "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/501", "steps": [{ "name": "Set up job", "status": "completed" }, { "name": "Flow tests", "status": "in_progress" }] });
    let queued = json!({ "id": 502, "name": "clis", "status": "queued", "conclusion": null, "started_at": null, "completed_at": null, "html_url": "https://github.com/o/r/actions/runs/77/job/502", "steps": [] });
    json!([preflight, running, queued])
}

fn live_github(comments: Value) -> Github {
    live_github_of(comments, live_jobs())
}

fn live_github_of(comments: Value, jobs: Value) -> Github {
    let github = Github::start();
    github.server.routes(&[
        ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(200, json!([{ "number": 7 }]))),
        (&format!("GET {PR_COMMENTS}?per_page=100&page=1"), MockReply::json(200, comments)),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({ "total_count": jobs.as_array().map_or(0, Vec::len), "jobs": jobs }))),
        ("GET /repos/o/r/actions/jobs/500/logs", MockReply::text(200, "ok\n")),
        (&format!("POST {PR_COMMENTS}"), MockReply::json(201, json!({ "id": 9100 }))),
        ("PATCH /repos/o/r/issues/comments/9001", MockReply::json(200, json!({ "id": 9001 }))),
        ("DELETE /repos/o/r/issues/comments/8000", MockReply::json(204, json!({}))),
        ("DELETE /repos/o/r/issues/comments/8001", MockReply::json(204, json!({}))),
    ]);
    github
}

#[test]
fn the_live_comment_of_a_new_run_replaces_the_old_one_at_the_bottom_and_shows_what_is_running() {
    // Given an earlier run's live comment on the pull request and a run with one job done, one running and one waiting
    let github = live_github(json!([{ "id": 8000, "body": "<!-- ci-live run:70 -->\n## 🟡 CI running" }, { "id": 8001, "body": "an unrelated comment" }]));

    // When the run's first job starts the live comment
    let r = github.ci_wf(&["live", "start"]);

    // Then the old one is deleted, a new one is created, and it names the job running with its step and the one waiting
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(github.sent("DELETE").len(), 1, "the earlier run's comment goes, an unrelated one stays");
    let created = github.sent("POST");
    assert_eq!(created.len(), 1, "{:?}", github.server.requests());
    let body = posted_body(&created[0]);
    assert!(body.contains("CI (self-hosted) running") && body.contains("1 running") && body.contains("1 waiting"), "{body}");
    assert!(body.contains("flow-tests") && body.contains("in: Flow tests") && body.contains("clis") && body.contains("waiting"), "{body}");
}

#[test]
fn the_live_comment_of_a_run_is_edited_in_place_not_recreated() {
    // Given this run's live comment already on the pull request
    let github = live_github(json!([{ "id": 9001, "body": "<!-- ci-live run:77 -->\nold" }]));
    // When a later job updates it
    let r = github.ci_wf(&["live"]);
    // Then the comment is edited and no second one is made
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((github.sent("PATCH").len(), github.sent("POST").len(), github.sent("DELETE").len()), (1, 0, 0), "{:?}", github.server.requests());
    assert!(posted_body(&github.sent("PATCH")[0]).contains("flow-tests"));
}

#[test]
fn a_run_with_no_pull_request_makes_no_live_comment() {
    let github = Github::start();
    github.server.routes(&[("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1", MockReply::json(200, json!([])))]);
    let r = github.ci_wf(&["live", "start"]);
    assert_eq!((r.exit_code, github.sent("POST").len()), (0, 0), "{}{}", r.stdout, r.stderr);
}

fn write_sized(path: &std::path::Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn age_days(path: &std::path::Path, days: u32) {
    let status = std::process::Command::new("touch").args(["-d", &format!("{days} days ago"), path.to_str().unwrap()]).status().unwrap();
    assert!(status.success());
}

#[test]
fn a_filling_runner_volume_loses_what_can_be_rebuilt_and_nothing_else_and_an_empty_one_loses_nothing() {
    // Given a runner volume holding a crate's cargo target (incremental state and built dependencies), five installs of wf of which the newest
    // three and the published one are to stay, and a file that is none of those
    let root = std::env::temp_dir().join(format!("tidy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_sized(&root.join("crate/Cargo.toml"), 10);
    write_sized(&root.join("crate/target/CACHEDIR.TAG"), 10);
    write_sized(&root.join("crate/target/debug/.fingerprint/f"), 10);
    write_sized(&root.join("crate/target/debug/incremental/state"), 4096);
    write_sized(&root.join("crate/target/debug/deps/lib.rlib"), 4096);
    // and a built artifact of a version of the code that nothing has touched for ten days, beside the fresh one
    write_sized(&root.join("crate/target/debug/deps/old-1a2b3c.rlib"), 4096);
    age_days(&root.join("crate/target/debug/deps/old-1a2b3c.rlib"), 10);
    write_sized(&root.join("notes.txt"), 10);
    let installs = root.join("installs");
    for (i, name) in ["t1", "t2", "t3", "t4", "t5"].iter().enumerate() {
        write_sized(&installs.join(name).join("wf"), 100);
        age_days(&installs.join(name), 10 - i as u32);
    }
    std::os::unix::fs::symlink("t1", installs.join("current")).unwrap();
    let run = |high: &str| {
        let r = run_wf(&["ci", "tidy", "--root", root.to_str().unwrap(), "--installs", installs.to_str().unwrap(), "--high", high, "--low", "0"], &ci_opts());
        (r.exit_code, last_json(&r))
    };

    // When the volume is under the high mark
    let (code, out) = run("100");
    // Then nothing is removed
    assert_eq!((code, out["removed"].as_array().unwrap().len()), (0, 0), "{out}");
    assert!(root.join("crate/target/debug/incremental/state").exists());

    // When it is over the mark
    let (code, out) = run("0");
    // Then the incremental state and the oldest installs the rule does not keep go, and the built dependencies, the files and the kept installs stay
    assert_eq!(code, 0, "{out}");
    assert!(!root.join("crate/target/debug/incremental").exists(), "{out}");
    assert!(root.join("crate/target/debug/deps/lib.rlib").exists() && root.join("notes.txt").exists());
    assert!(!root.join("crate/target/debug/deps/old-1a2b3c.rlib").exists(), "the artifact nothing used for ten days goes while the fresh one stays: {out}");
    let kept: Vec<bool> = ["t1", "t2", "t3", "t4", "t5"].iter().map(|n| installs.join(n).exists()).collect();
    assert_eq!(kept, [true, false, true, true, true], "the published install and the newest three stay: {out}");
    let _ = std::fs::remove_dir_all(&root);
}

const SICK_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=warn source=ci-cache msg=\"cache.run\" kind=nx hits=3 misses=1 puts=4 skipped=0 requests=9 bytes=10 degraded=no seeded_checked=4 seeded_present=3
2026-10-23T10:00:09.0000000Z ts=1793000001 level=warn source=ci-cache msg=\"cache.put.dropped\" kind=sccache dropped=3 quota_exceeded=3 error_class=quota-exceeded error_status=400
2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=75.2
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=5 replayed=0 replay_ratio=0.00
2026-10-23T10:00:12.0000000Z ts=1793000004 level=warn source=ci-cache msg=\"cache.shim.disabled\" reason=\"WF_BLOB_CACHE=off\"
";

const HEALTHY_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=info source=ci-cache msg=\"cache.run\" kind=nx hits=3 misses=1 puts=4 skipped=0 requests=9 bytes=10 degraded=no seeded_checked=4 seeded_present=4
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=0 replayed=5 replay_ratio=1.00
";

fn health_wf(github: &Github, event: &str) -> RunResult {
    health_wf_with(github, event, &[])
}

fn health_wf_with(github: &Github, event: &str, extra: &[(&str, &str)]) -> RunResult {
    let mut opts = ci_opts()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_RUN_ID", "77")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_API_URL", &github.url())
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REF_NAME", "dev")
        .env("GITHUB_EVENT_NAME", event)
        .env("WF_CI_LOG_CACHE", &log_cache())
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    for (k, v) in extra {
        opts = opts.env(k, v);
    }
    run_wf(&["ci", "cache-health"], &opts)
}

#[test]
fn the_cache_health_verb_fails_a_run_whose_layers_lost_or_refused_writes_and_passes_a_healthy_one() {
    // Given a green run whose flow-tests job logged a lost write, refused sccache writes, a 75 s pnpm save, a stage that replayed nothing
    // and a disabled shim
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), SICK_LOG);

    // When the health of a push to dev is asked
    let r = health_wf(&github, "push");

    // Then it exits 1 and names each problem with its job, and the disabled cache is a note
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (1, json!(false)), "{}{}", r.stdout, r.stderr);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["pnpm-save-slow", "rejected-write", "replayed-nothing", "seeded-lost"], "{out}");
    let seeded = out["findings"].as_array().unwrap().iter().find(|f| f["kind"] == "seeded-lost").unwrap();
    assert!(seeded["job"] == "flow-tests" && seeded["detail"].as_str().unwrap().contains("seeded 3 of 4"), "{out}");
    assert!(out["notes"][0].as_str().unwrap().contains("WF_BLOB_CACHE=off"), "{out}");

    // When the same logs belong to a run that is not a push to dev
    let other = health_wf(&github, "pull_request");
    // Then a stage that replayed nothing is no problem there, the others still are
    let kinds: Vec<String> = last_json(&other)["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["pnpm-save-slow", "rejected-write", "seeded-lost"]);

    // Given a run whose cache did everything it should
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), HEALTHY_LOG);
    let healthy = health_wf(&github, "push");
    // Then it exits 0 with the stages it saw
    let out = last_json(&healthy);
    assert_eq!((healthy.exit_code, out["ok"].clone(), out["findings"].clone()), (0, json!(true), json!([])), "{}{}", healthy.stdout, healthy.stderr);
    assert_eq!(out["stages"][0], json!({ "job": "flow-tests", "stage": "flow-tests", "replayed": 5, "tasks": 5 }));
}

fn typecheck_log(task: &str) -> String {
    format!("2026-10-23T10:00:08.0000000Z wf: nx ledger {{\"tasks\":[{{\"task\":\"{task}\",\"hash\":\"12495670247005852844\",\"outcome\":\"ran\",\"ms\":1260,\"passed\":true}}]}}
2026-10-23T10:00:09.0000000Z ts=1793000001 level=info source=ci-cache msg=\"nx.stage.summary\" stage=typecheck-lint-unit-tests target=typecheck ran=1 replayed=0 replay_ratio=0.00
")
}

#[test]
fn the_cache_health_verb_does_not_blame_the_gate_for_a_docs_only_push_whose_only_task_is_the_workspace_root_typecheck() {
    // Given a push to dev that changed only docs, so the stage's one task is the workspace root's typecheck and it ran, not replayed
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "typecheck-lint-unit-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), &typecheck_log("expo-hello-world-app-2-workspace:typecheck"));

    // When the health of the push is asked
    let r = health_wf(&github, "push");

    // Then no replayed-nothing finding is raised, and the stage is still reported
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["findings"].clone()), (0, json!([])), "{}{}", r.stdout, r.stderr);
    assert_eq!(out["stages"][0], json!({ "job": "typecheck-lint-unit-tests", "stage": "typecheck-lint-unit-tests", "replayed": 0, "tasks": 1 }));

    // Given the same stage whose one task is a package's typecheck, which the gate does seed
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), &typecheck_log("mla-tamagui:typecheck"));

    // When the health is asked again
    let blamed = health_wf(&github, "push");

    // Then the stage that replayed nothing is a replayed-nothing finding
    let out = last_json(&blamed);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!((blamed.exit_code, kinds), (1, vec!["replayed-nothing"]), "{out}");

    // Given a repo whose pre-push hook seeds nothing (WF_CI_PUSH_GATE=off), and the same stage that replayed nothing
    let unseeded = health_wf_with(&github, "push", &[("WF_CI_PUSH_GATE", "off")]);

    // Then there is no replayed-nothing finding to blame the gate for
    let out = last_json(&unseeded);
    assert_eq!((unseeded.exit_code, out["findings"].clone()), (0, json!([])), "{}{}", unseeded.stdout, unseeded.stderr);
}

const COLD_LOG: &str = "2026-10-23T10:00:08.0000000Z ts=1793000000 level=warn source=ci-cache msg=\"cache.run\" kind=sccache hits=0 misses=0 puts=0 skipped=9 requests=0 bytes=0 degraded=unreachable seeded_checked=0 seeded_present=0
2026-10-23T10:00:09.0000000Z ts=1793000001 level=info source=ci-cache msg=\"cache.save\" kind=pnpm verb=save done=true bytes=100 reason=\"\" seconds=3.0 error_class=rejected error_status=503
2026-10-23T10:00:10.0000000Z ts=1793000002 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=cargo-flow ran=4 replayed=0 replay_ratio=0.00
2026-10-23T10:00:11.0000000Z ts=1793000003 level=info source=ci-cache msg=\"nx.stage.summary\" stage=flow-tests target=test:flow ran=0 replayed=6 replay_ratio=1.00
2026-10-23T10:00:12.0000000Z ts=1793000004 level=warn source=ci-cache msg=\"drift.report\" compared=10 drifted=2 causes=\"env\"
wf: build cache {\"degraded\":\"budget\",\"droppedPuts\":0}
";

#[test]
fn the_cache_health_verb_names_a_cold_layer_and_drift_and_judges_a_stage_by_all_its_kinds_and_a_save_by_what_it_stored() {
    // Given a push to dev whose logs hold a cold sccache, a budget-degraded shim line, a drift report, a pnpm save that stored fine
    // after a 503 on an earlier lookup, and a stage whose cargo kind replayed nothing while its other kind replayed everything
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), COLD_LOG);

    // When the health is asked
    let r = health_wf(&github, "push");

    // Then the cold layer and the drift are findings, and neither the stored save nor the half-replayed stage is
    let out = last_json(&r);
    let kinds: Vec<&str> = out["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!((r.exit_code, kinds), (1, vec!["degraded", "drift"]), "{out}");
    let degraded = out["findings"][0]["detail"].as_str().unwrap();
    assert!(degraded.contains("unreachable"), "{out}");
    assert!(out["findings"][1]["detail"].as_str().unwrap().contains("2 of 10"), "{out}");
    assert_eq!(out["stages"][0], json!({ "job": "flow-tests", "stage": "flow-tests", "replayed": 6, "tasks": 10 }));
}

#[test]
fn the_cache_health_verb_does_not_fail_a_run_it_could_not_read() {
    // Given a GitHub API that is gone
    let opts = ci_opts()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_RUN_ID", "77")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_API_URL", "http://127.0.0.1:1")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REF_NAME", "dev")
        .env("GITHUB_EVENT_NAME", "push")
        .env("WF_CI_LOG_CACHE", &log_cache())
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");

    // When the health is asked
    let r = run_wf(&["ci", "cache-health"], &opts);

    // Then the verdict says it could not be computed and the exit stays 0
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (0, json!(null)), "{}{}", r.stdout, r.stderr);
    assert!(out["reason"].is_string(), "{out}");
}

#[test]
fn the_live_comment_has_a_cache_health_section_naming_the_problems_and_a_healthy_run_says_so() {
    // Given a green run whose flow-tests job lost a write, and one whose cache was healthy
    let github = Github::start();
    let jobs = json!([job(500, "plan", "success", "2026-10-23T10:00:20Z", json!([])), job(501, "flow-tests", "success", "2026-10-23T10:02:00Z", json!([]))]);
    let summary = std::env::temp_dir().join(format!("ci-summary-health-{}", std::process::id()));
    let _ = std::fs::remove_file(&summary);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), SICK_LOG);

    // When wf reports
    let r = github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", summary.to_str().unwrap())]);

    // Then the comment has the section with each problem and the job it came from
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report = std::fs::read_to_string(&summary).unwrap();
    assert!(report.contains("### 🗄️ Cache health") && report.contains("problem(s)"), "{report}");
    assert!(report.contains("seeded 3 of 4") && report.contains("| rejected-write | flow-tests |") && report.contains("disabled in flow-tests"), "{report}");

    // And a healthy run says so
    let clean = std::env::temp_dir().join(format!("ci-summary-health-ok-{}", std::process::id()));
    let _ = std::fs::remove_file(&clean);
    github.serve_log(&jobs, json!([]), json!([]), created(), updated(), HEALTHY_LOG);
    github.comment_wf_with(&[], &[("GITHUB_STEP_SUMMARY", clean.to_str().unwrap())]);
    let report = std::fs::read_to_string(&clean).unwrap();
    assert!(report.contains("🟢 Healthy") && report.contains("Nx replayed 5 of 5"), "{report}");
}

#[test]
fn a_labelled_pipeline_keeps_its_own_titled_comment_and_never_deletes_the_cis() {
    // Given the CI's live comment and an earlier run's comment of the labelled pipeline on the pull request
    let github = live_github(json!([
        { "id": 8000, "body": "<!-- ci-live run:70 -->\n## 🟡 CI running" },
        { "id": 8001, "body": "<!-- ci-live:github-hosted run:70 -->\n## 🟡 CI (GitHub-hosted) running" },
    ]));

    // When the labelled pipeline's run starts its live comment
    let r = github.ci_wf_with(&["live", "start"], &[("WF_CI_LABEL", "GitHub-hosted")]);

    // Then only its own earlier comment is deleted, and the new one carries the label in its marker and its title
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let deleted: Vec<String> = github.sent("DELETE").iter().map(|d| d.url.clone()).collect();
    assert_eq!(deleted, ["/repos/o/r/issues/comments/8001"], "the CI's comment stays: {:?}", github.server.requests());
    let created = github.sent("POST");
    assert_eq!(created.len(), 1);
    let body = posted_body(&created[0]);
    assert!(body.starts_with("<!-- ci-live:github-hosted run:77 -->"), "{body}");
    assert!(body.contains("## 🟡 CI (GitHub-hosted) running") || body.contains("CI (GitHub-hosted)"), "{body}");

    // and the CI's own run removes the CI's comment only
    let github = live_github(json!([
        { "id": 8000, "body": "<!-- ci-live run:70 -->\n## 🟡 CI running" },
        { "id": 8001, "body": "<!-- ci-live:github-hosted run:70 -->\n## 🟡 CI (GitHub-hosted) running" },
    ]));
    let r = github.ci_wf(&["live", "start"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let deleted: Vec<String> = github.sent("DELETE").iter().map(|d| d.url.clone()).collect();
    assert_eq!(deleted, ["/repos/o/r/issues/comments/8000"], "{:?}", github.server.requests());
}

#[test]
fn a_labelled_pipeline_reports_its_own_runners_numbers_instead_of_the_vms() {
    // Given a labelled run whose jobs left their sampled runner numbers in a directory
    let dir = std::env::temp_dir().join(format!("wf-runner-stats-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("stats-flow-tests.jsonl"),
        "{\"t\":1,\"cpu\":40,\"memUsedMb\":2000,\"memTotalMb\":7000,\"load1\":1.0}\n{\"t\":6,\"cpu\":90,\"memUsedMb\":3000,\"memTotalMb\":7000,\"load1\":2.5}\nnot json\n",
    )
    .unwrap();
    let github = live_github(json!([]));

    // When the run's live comment is built
    let r = github.ci_wf_with(&["live", "start"], &[("WF_CI_LABEL", "GitHub-hosted"), ("WF_CI_RUNNER_STATS_DIR", dir.to_str().unwrap())]);

    // Then the comment has the runners table with the job's averages and peaks, and no VM history
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let created = github.sent("POST");
    assert_eq!(created.len(), 1, "{:?}", github.server.requests());
    let body = posted_body(&created[0]);
    assert!(body.contains("### 🖥️ Runners during the run"), "{body}");
    assert!(body.contains("| 🔴 flow-tests | 65% | **90%** | 2.5 GB | **3.0 GB** (43%) | 2.5 | 2 |"), "{body}");
    assert!(!body.contains("VM during the run"), "{body}");
    std::fs::remove_dir_all(&dir).ok();
}

/// The jobs of a run of the shared workflow: the Actions API names each `<caller job> / <job>`, and a sharded one `(i/n)` after its name.
fn called_run() -> Value {
    let steps = json!([{ "name": "Set up job", "conclusion": "success" }, { "name": "Flow tests", "conclusion": "failure" }]);
    json!([
        job(500, "run / plan", "success", "2026-10-23T10:00:20Z", json!([])),
        job(501, "run / flow-tests (2/4)", "failure", "2026-10-23T10:03:05Z", steps),
        job(502, "run / deploy (vm-ops-api)", "skipped", "2026-10-23T10:03:06Z", json!([])),
        job(503, "run / report", "failure", "2026-10-23T10:03:07Z", json!([])),
        job(504, "run / budget", "failure", "2026-10-23T10:03:07Z", json!([]))
    ])
}

#[test]
fn the_report_of_a_called_workflow_names_a_job_without_the_caller_prefix_and_leaves_its_own_report_jobs_out() {
    // Given a run whose jobs the API names `run / <job>`, a sharded job failed, and the report and budget jobs of the run are red too
    let github = Github::start();
    github.serve(&called_run(), json!([]), created(), updated());

    // When the comment is previewed
    let r = github.comment_wf(&["--dry-run"]);

    // Then the failed job is the sharded one under its own name, and neither report job counts as a failure
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["failedJobs"].clone()), (0, json!(["flow-tests (2/4)"])), "{}{}", r.stdout, r.stderr);
    let body = out["body"].as_str().unwrap();
    assert!(body.contains("flow-tests (2/4)") && body.contains("`deploy (vm-ops-api)`"), "{body}");
    assert!(!body.contains("run / "), "the caller's prefix is not shown: {body}");
}

#[test]
fn the_live_comment_of_a_called_workflow_shows_a_sharded_job_by_its_own_name() {
    // Given a run whose running job is shard 2 of 4 of flow-tests, named behind the caller's job
    let mut jobs = live_jobs();
    jobs[0]["name"] = json!("run / preflight");
    jobs[1]["name"] = json!("run / flow-tests (2/4)");
    jobs[2]["name"] = json!("run / clis");
    let github = live_github_of(json!([]), jobs);

    // When the live comment starts
    let r = github.ci_wf(&["live", "start"]);

    // Then it names the running shard and the waiting job without the prefix
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let body = posted_body(&github.sent("POST")[0]);
    assert!(body.contains("flow-tests (2/4)") && body.contains("in: Flow tests") && body.contains("clis"), "{body}");
    assert!(!body.contains("run / "), "{body}");
}

#[test]
fn the_audit_of_a_called_workflow_judges_a_job_by_its_own_name_and_never_judges_its_report_jobs() {
    // Given a run of the shared workflow where a sharded job waited fifteen minutes for a runner, and the run's report job waited as long
    let mut waited = timed_job(500, "run / flow-tests (2/4)", "success", 1500);
    waited["started_at"] = json!(iso(BASE + 900));
    let mut report = timed_job(501, "run / report", "success", 1500);
    report["started_at"] = json!(iso(BASE + 900));
    let github = audit_github_of(&[waited, report], "ok\n", &[]);

    // When the audit judges the run
    let r = Audit::new("called", &[], "", json!({})).run(&github, true);

    // Then only the sharded job's wait is reported, under its own name
    let out = last_json(&r);
    let waits: Vec<&Value> = out["findings"].as_array().unwrap().iter().filter(|b| b["kind"] == "runner-wait").collect();
    assert_eq!(waits.len(), 1, "{out}");
    assert_eq!(waits[0]["subject"], "flow-tests (2/4)", "{out}");
}
