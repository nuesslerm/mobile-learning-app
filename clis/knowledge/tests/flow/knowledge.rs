//! The flow tests of `wf knowledge`: the gaps and learnings ledger (`rank`, `index`, `stale`) over a throwaway repository,
//! and the Gemini bundle (`index --gemini`) through fake `pnpm`/`repomix` and `wf offload ask` stubs, through the thin `knowledge` binary.
use crate::support::{iso_from_ms, now_ms, run_wf, Gem, RunOpts};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

#[derive(Default)]
struct Gap<'a> {
    status: Option<&'a str>,
    area: &'a str,
    opened: Option<&'a str>,
    verified: Option<&'a str>,
    verify: Option<&'a str>,
    title: Option<&'a str>,
}

fn gap(o: Gap) -> String {
    format!(
        "---\nstatus: {}\narea: {}\npaths: [infra/**]\nopened: {}\nlast_verified: {}\n{}links: []\n---\n# {}\n\n## What is broken\nx\n",
        o.status.unwrap_or("open"),
        o.area,
        o.opened.unwrap_or("2026-09-01"),
        o.verified.unwrap_or("2026-09-01"),
        o.verify.map_or(String::new(), |v| format!("verify: {v}\n")),
        o.title.unwrap_or("A gap")
    )
}

fn learning(title: Option<&str>, date: &str, category: Option<&str>, paths: &str) -> String {
    format!("---\n{}date: {date}\n{}tags: [a]\npaths: [{paths}]\n---\n# Heading\n\nbody\n", title.map_or(String::new(), |t| format!("title: {t}\n")), category.map_or(String::new(), |c| format!("category: {c}\n")))
}

#[test]
fn knowledge_ranks_the_gaps_and_learnings_a_file_touches_indexes_the_gaps_and_finds_the_stale_ones() {
    let new_repo = || {
        let dir = tempfile::Builder::new().prefix("wf-flow-knowledge-").tempdir().unwrap();
        assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(dir.path()).status().unwrap().success());
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        dir
    };
    let put = |root: &Path, rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let knowledge = |root: &Path, args: &[&str]| {
        let mut full = vec!["knowledge"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().cwd(root))
    };

    // Given a gap and learnings with path globs, When a touched file is ranked, Then the gap comes first, then learnings by recency
    let r = new_repo();
    let root = r.path();
    put(root, "docs/gaps/vm-down.md", &gap(Gap { area: "vm", title: Some("VM is down"), ..Default::default() }));
    put(root, "docs/gaps/README.md", "ignored");
    put(root, "docs/solutions/workflow/lesson.md", &learning(Some("A lesson"), "2026-09-02", Some("workflow"), "\"infra/**\""));
    put(root, "docs/solutions/workflow/older.md", &learning(None, "2026-08-01", None, "\"infra/oracle-cloud/\""));
    put(root, "docs/solutions/workflow/plain.md", "# plain\n");
    put(root, "docs/solutions/other.md", &learning(None, "2026-09-09", None, "frontend/**/*.tsx"));
    let out = knowledge(root, &["rank", "infra/oracle-cloud/main.tf"]);
    assert_eq!((out.exit_code, out.stdout.as_str()), (0, "- gap (open, vm): [VM is down](docs/gaps/vm-down.md)\n- learning (workflow): [A lesson](docs/solutions/workflow/lesson.md)\n- learning: [Heading](docs/solutions/workflow/older.md)\n"));
    // and --limit keeps the top hit, a ./ prefix is ignored, a recursive glob matches nested and top-level files, and no match prints nothing
    assert_eq!(knowledge(root, &["rank", "infra/x.tf", "--limit", "1"]).stdout, "- gap (open, vm): [VM is down](docs/gaps/vm-down.md)\n");
    assert!(knowledge(root, &["rank", "./infra/x.tf", "--limit", "1"]).stdout.contains("VM is down"));
    assert_eq!(knowledge(root, &["rank", "frontend/a/b/c.tsx"]).stdout, "- learning: [Heading](docs/solutions/other.md)\n");
    assert_eq!(knowledge(root, &["rank", "frontend/c.tsx"]).stdout, "- learning: [Heading](docs/solutions/other.md)\n");
    assert_eq!(knowledge(root, &["rank", "frontend/c.ts"]).stdout, "");
    assert_eq!((knowledge(root, &["rank", "nowhere/x"]).stdout, knowledge(root, &["rank", "nowhere/x"]).exit_code), (String::new(), 0));
    // and an invalid gap does not stop the ranking
    put(root, "docs/gaps/bad.md", "---\nstatus: nope\n---\n# t\n");
    assert_eq!(knowledge(root, &["rank", "infra/x"]).exit_code, 0);

    // Given gaps in two areas, When indexed, Then the lines sort by area then opened date under the fixed header
    let r = new_repo();
    let root = r.path();
    assert!(knowledge(root, &["index"]).stdout.ends_with("\nNo open gaps.\n"));
    put(root, "docs/gaps/b.md", &gap(Gap { area: "wf", title: Some("B"), opened: Some("2026-09-01"), ..Default::default() }));
    put(root, "docs/gaps/a.md", &gap(Gap { area: "vm", title: Some("A"), status: Some("blocked"), opened: Some("2026-09-20"), ..Default::default() }));
    put(root, "docs/gaps/c.md", &gap(Gap { area: "wf", title: Some("C"), opened: Some("2026-08-01"), ..Default::default() }));
    let out = knowledge(root, &["index"]);
    assert_eq!(out.exit_code, 0);
    let lines: Vec<&str> = out.stdout.split('\n').collect();
    assert_eq!(
        lines[..6],
        [
            "# Known gaps",
            "",
            "Generated by `wf knowledge index --write` from `docs/gaps/*.md`; do not edit by hand.",
            "One line per open gap: `area · title · age · link`. A gap closes only through `/cf-compound <slug>`.",
            "Pre-2026-10 history: [archive](archive/README.md).",
            "",
        ]
    );
    for (line, re) in lines[6..9].iter().zip([r"^- vm · A \(blocked\) · \d+d · \[a\]\(gaps/a\.md\)$", r"^- wf · C · \d+d · \[c\]\(gaps/c\.md\)$", r"^- wf · B · \d+d · \[b\]\(gaps/b\.md\)$"]) {
        assert!(Regex::new(re).unwrap().is_match(line), "{line}");
    }
    assert_eq!(lines[9], "");
    // and --write writes it and counts it
    assert_eq!(knowledge(root, &["index", "--write"]).stdout, "wrote docs/known-gaps.md: 3 gaps, 9 lines\n");
    assert!(std::fs::read_to_string(root.join("docs/known-gaps.md")).unwrap().contains("- vm · A (blocked)"));

    // Given more than 200 gaps, Then --write fails and writes nothing
    let r = new_repo();
    let root = r.path();
    for i in 0..201 {
        put(root, &format!("docs/gaps/g{i}.md"), &gap(Gap { area: "vm", title: Some(&format!("G{i}")), ..Default::default() }));
    }
    let out = knowledge(root, &["index", "--write"]);
    assert!(out.exit_code == 1 && out.stderr.contains("docs/known-gaps.md would be 207 lines (> 200): close or merge gaps\n") && !root.join("docs/known-gaps.md").exists(), "{}", out.stderr);

    // Given invalid gap records, Then index and stale list each error and exit 1
    let r = new_repo();
    let root = r.path();
    put(root, "docs/gaps/bad.md", "---\nstatus: nope\narea: \" \"\npaths: []\nopened: 2026-9-1\nverify: 5\n---\nno title\n");
    put(root, "docs/gaps/nofm.md", "# nothing\n");
    put(root, "docs/gaps/ok.md", &gap(Gap { area: "vm", ..Default::default() }));
    let out = knowledge(root, &["index"]);
    assert_eq!((out.exit_code, out.stdout.as_str()), (1, ""));
    assert!(out.stderr.contains("invalid gap records:\n  docs/gaps/bad.md: status must be open|blocked, got \"nope\"; area is required; paths must list at least one glob; opened must be YYYY-MM-DD; last_verified must be YYYY-MM-DD; verify must be a command string; missing `# Title` heading\n  docs/gaps/nofm.md: missing frontmatter\n"), "{}", out.stderr);
    assert_eq!(knowledge(root, &["stale"]).exit_code, 1);
    let r = new_repo();
    let root = r.path();
    put(root, "docs/gaps/closed-with-colon.md", "---\nstatus: closed\narea: deploy\npaths: [x]\nopened: 2026-10-06\nlast_verified: 2026-10-06\nclosed: 2026-10-06 (moved behind the installs: 68 s build)\nlinks: []\n---\n# T\n");
    put(root, "docs/gaps/ok.md", &gap(Gap { area: "vm", ..Default::default() }));
    let out = knowledge(root, &["index"]);
    assert!(out.exit_code == 1 && out.stdout.is_empty() && out.stderr.contains("invalid gap records:\n  docs/gaps/closed-with-colon.md: frontmatter is not valid YAML") && !root.join("docs/known-gaps.md").exists(), "{}", out.stderr);
    put(root, "docs/gaps/closed-with-colon.md", "---\narea: [unclosed\n---\n# T\n");
    let out = knowledge(root, &["stale"]);
    assert!(out.exit_code == 1 && out.stderr.contains("invalid gap records:\n  docs/gaps/closed-with-colon.md: frontmatter is not valid YAML"), "{}", out.stderr);
    put(root, "docs/gaps/closed-with-colon.md", "---\narea: vm\npaths: [x]\nopened: 2026-09-01\nlast_verified: 2026-09-01\n---\n# T\n");
    assert!(knowledge(root, &["index"]).stderr.contains("status must be open|blocked, got undefined"));

    // Given old and fresh gaps, Then stale prints only the old ones, oldest first, past the --days threshold
    let r = new_repo();
    let root = r.path();
    put(root, "docs/gaps/old.md", &gap(Gap { area: "vm", verified: Some("2026-01-02"), verify: Some("curl -sf x"), ..Default::default() }));
    put(root, "docs/gaps/older.md", &gap(Gap { area: "vm", verified: Some("2025-12-01"), ..Default::default() }));
    let today = iso_from_ms(now_ms())[..10].to_string();
    put(root, "docs/gaps/fresh.md", &gap(Gap { area: "vm", verified: Some(&today), ..Default::default() }));
    assert_eq!((knowledge(root, &["stale"]).exit_code, knowledge(root, &["stale"]).stdout), (0, "older\t2025-12-01\t-\nold\t2026-01-02\tcurl -sf x\n".to_string()));
    assert_eq!(knowledge(root, &["stale", "--days", "100000"]).stdout, "");
    let out = knowledge(root, &["stale", "--days", "x"]);
    assert!(out.exit_code == 2 && out.stderr.contains("--days must be a positive integer\n"));
}

#[test]
fn knowledge_builds_one_bundle_with_the_architecture_first_and_a_dry_run_uploads_and_records_nothing() {
    // Given a repository whose model answers with an architecture document
    let g = Gem::new(&["# Architecture\\nui to domain"]);
    // When the bundle is built with --dry-run
    let out = g.index(&["--dry-run"]);
    // Then ARCHITECTURE.md is the first Doc, the full pack the next, and no manifest is written
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(g.cached("docs/gem-00-architecture.md"), "# Architecture\nui to domain\n");
    assert!(g.cached("docs/gem-01.md").starts_with("## File: a.ts\nPACK\n"), "{}", g.cached("docs/gem-01.md"));
    assert!(out.stdout.contains("2 Docs,") && out.stdout.contains(", 3 files, architecture "), "{}", out.stdout);
    assert!(!g.repo.join("docs/gemini-context/manifest.json").exists());
    assert_eq!(g.asks(), 1);

    // When it is rebuilt with --no-summary, Then the cached architecture is reused and the model is not asked
    assert_eq!(g.index(&["--dry-run", "--no-summary"]).exit_code, 0);
    assert_eq!(g.asks(), 1);
    // and with nothing cached, --no-summary refuses
    std::fs::remove_file(g.repo.join(".gemini-context-cache/ARCHITECTURE.md")).unwrap();
    let out = g.index(&["--dry-run", "--no-summary"]);
    assert!(out.exit_code == 1 && out.stderr.contains("--no-summary needs a cached"), "{}", out.stderr);
}

#[test]
fn knowledge_retries_an_unusable_architecture_and_gives_up_after_three_without_a_bundle() {
    // Given a model that answers nothing, then WROTE_NOTHING, then prose without a heading
    let g = Gem::new(&["", "WROTE_NOTHING", "just words"]);
    // When the bundle is built
    let out = g.index(&["--dry-run"]);
    // Then three attempts were made, the run fails naming the cause, and no bundle exists
    assert_eq!(out.exit_code, 1);
    assert!(out.stderr.contains("ARCHITECTURE.md could not be drafted after 3 attempts"), "{}", out.stderr);
    assert_eq!((g.asks(), g.cached("docs/gem-00-architecture.md")), (3, String::new()));

    // Given a first unusable answer and then a good one, Then the second attempt is used
    let g = Gem::new(&["", "# Architecture\\nok"]);
    assert_eq!(g.index(&["--dry-run"]).exit_code, 0);
    assert_eq!(g.asks(), 2);
    assert!(g.cached("docs/gem-00-architecture.md").starts_with("# Architecture\nok\n"));
}

#[test]
fn knowledge_needs_a_destination_to_upload_and_the_surface_has_no_locate() {
    // Given no remote and no --dry-run, Then it refuses before building anything
    let g = Gem::new(&["# Architecture"]);
    let out = g.index(&[]);
    assert!(out.exit_code == 1 && out.stderr.contains("no destination: set GEMINI_CONTEXT_REMOTE or pass --remote"), "{}", out.stderr);
    assert_eq!(g.asks(), 0);
    // and a malformed remote is named
    let out = g.index(&["--remote", "nofolder"]);
    assert!(out.exit_code == 1 && out.stderr.contains("must be <drive-folder-id>/<doc-name-prefix> or local:<dir>/<doc-name-prefix>"), "{}", out.stderr);

    // Then the retired locate verb is gone from the help
    let help = run_wf(&["knowledge", "--help"], &RunOpts::default().bin_dir(&g.bin).cwd(&g.repo).clean_env());
    assert!(help.exit_code == 0 && !help.stdout.contains("locate"), "{}", help.stdout);
}

#[test]
fn knowledge_overwrites_the_docs_in_place_records_the_upload_and_a_failed_upload_changes_nothing() {
    // Given a local remote holding an earlier first part
    let g = Gem::new(&["# Architecture\\nfirst"]);
    let remote_dir = g.dir.path().join("remote");
    std::fs::create_dir(&remote_dir).unwrap();
    std::fs::write(remote_dir.join("ctx-01.md"), "old").unwrap();
    let remote = format!("local:{}/ctx", remote_dir.display());
    // When the Docs are uploaded
    let out = g.index(&["--remote", &remote]);
    // Then the architecture Doc and the pack Doc hold the new text and the first part was overwritten, not duplicated
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(std::fs::read_to_string(remote_dir.join("ctx-00-architecture.md")).unwrap(), "# Architecture\nfirst\n");
    assert!(std::fs::read_to_string(remote_dir.join("ctx-01.md")).unwrap().starts_with("## File: a.ts"));
    assert_eq!(std::fs::read_dir(&remote_dir).unwrap().count(), 2);
    // and each Doc was read back: one proof line per Doc with its character count
    assert!(out.stdout.contains("doc ctx-00-architecture id=") && out.stdout.contains("doc ctx-01 id="), "{}", out.stdout);
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(g.repo.join("docs/gemini-context/manifest.json")).unwrap()).unwrap();
    assert_eq!((manifest["version"].clone(), manifest["remote"].clone(), manifest["uploaded"].clone()), (json!(3), json!(remote), json!(true)));
    assert_eq!(manifest["docs"].as_array().unwrap().len(), 2);

    // When the destination cannot be written, Then the run fails naming it and the manifest keeps its last record
    let before = std::fs::read_to_string(g.repo.join("docs/gemini-context/manifest.json")).unwrap();
    std::fs::write(g.dir.path().join("blocker"), "a file, not a directory").unwrap();
    let out = g.index(&["--no-summary", "--remote", &format!("local:{}/blocker/sub/ctx", g.dir.path().display())]);
    assert!(out.exit_code == 1 && out.stderr.contains("blocker"), "{}", out.stderr);
    assert_eq!(std::fs::read_to_string(g.repo.join("docs/gemini-context/manifest.json")).unwrap(), before);
}

#[test]
fn knowledge_splits_a_pack_over_one_doc_into_parts_and_refuses_one_that_needs_more_than_ten_docs() {
    // Given a pack of three files of 400k characters each
    let g = Gem::new(&["# Architecture"]);
    let file = |n: &str| format!("## File: {n}\n{}\n", "x".repeat(400_000));
    g.set_pack(&format!("{}{}{}", file("a.ts"), file("b.ts"), file("c.ts")));
    // When the Docs are built
    let out = g.index(&["--dry-run"]);
    // Then the architecture is Doc 00 and the pack fills Docs 01 and 02, none over 1,000k characters
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let (p1, p2) = (g.cached("docs/gem-01.md"), g.cached("docs/gem-02.md"));
    assert!(!p1.is_empty() && !p2.is_empty() && g.cached("docs/gem-03.md").is_empty());
    assert!(p1.chars().count() <= 1_000_000 && p2.chars().count() <= 1_000_000);

    // Given a pack that needs eleven parts, Then the run refuses and names the Gem's limit
    let big: String = (0..11).map(|i| format!("## File: f{i}.ts\n{}\n", "y".repeat(800_000))).collect();
    g.set_pack(&big);
    let out = g.index(&["--dry-run", "--no-summary"]);
    assert!(out.exit_code == 1 && out.stderr.contains("a Gem takes 10"), "{}", out.stderr);
}
