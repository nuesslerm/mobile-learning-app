//! `wf check --dead-refs`: the scan for references to code that is gone (the staged gate and the budget are in `pipeline`, the
//! verification of a deployed commit in `deploy`). The repository is a real throwaway git.
use crate::support::{run_wf, RunOpts};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn dead_refs_names_each_path_link_package_command_verb_method_and_script_that_is_gone_and_passes_a_clean_tree() {
    let repo = tempfile::Builder::new().prefix("wf-flow-dead-refs-").tempdir().unwrap();
    let root = repo.path();
    git(root, &["init", "-q"]);
    put(root, "libraries/real/package.json", r#"{ "name": "@lib/real" }"#);
    put(root, "clis/real/src/lib.rs", "// here\n");
    put(root, "package.json", r#"{ "name": "root", "scripts": { "real": "true" } }"#);
    put(root, "docs/solutions/b.md", "b\n");
    // Given a doc naming a deleted file, a removed package, a moved doc, a removed script and, in a fenced block, a renamed
    // command, and a script calling that renamed command
    put(root, "docs/solutions/a.md", "See `clis/real/src/lib.rs` and `clis/gone/src/x.ts`; import `@lib/real` and `@lib/gone`.\nRun `pnpm run real` and `pnpm run gone`; read [b](b.md) and [c](c.md).\n```bash\nwf secret get KEY\nwf secrets get KEY\n```\n");
    put(root, "scripts/run.sh", "#!/bin/sh\nwf secret get KEY\nwf secrets get KEY\n");
    // and a runbook naming device verbs and Lua methods that route, one verb no noun has, a method from before the rename, a
    // removed one and one that never was
    put(root, "docs/runbooks/r.md", "Tap with `wf android tap-on --point 1,2` or `wf browser tap-on Go`, not `wf browser app-flows`.\n```lua\ndevice:tap_on('Go')\ndevice:find('id:Go')\ndevice:stroke({})\ndevice:bogus()\n```\n");
    // and a goal whose verifier calls a verb that is gone, beside a path it once named
    put(root, "docs/goals/g.md", "```bash\nwf android test --api | grep -q 'device:assert_screenshot'\nwf android bogus-verb\n```\n`clis/gone/src/x.ts`\n");
    // history is not read
    put(root, "docs/archive/old.md", "`clis/gone/src/x.ts` and `@lib/gone` and `wf secrets get`\n");
    git(root, &["add", "-A"]);
    let check = || run_wf(&["check", "--dead-refs"], &RunOpts::default().cwd(root));

    // When the tree is checked
    let r = check();

    // Then each dead reference is named once, with where it is, and nothing else
    assert_eq!(r.exit_code, 1, "{}\n{}", r.stdout, r.stderr);
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    let found: Vec<(String, u64, String, String)> = v["findings"].as_array().unwrap().iter().map(|f| (f["file"].as_str().unwrap().into(), f["line"].as_u64().unwrap(), f["kind"].as_str().unwrap().into(), f["reference"].as_str().unwrap().into())).collect();
    assert_eq!(found, vec![
        ("docs/goals/g.md".into(), 3, "verb".into(), "wf android bogus-verb".into()),
        ("docs/runbooks/r.md".into(), 1, "verb".into(), "wf browser app-flows".into()),
        ("docs/runbooks/r.md".into(), 4, "method".into(), "device:find".into()),
        ("docs/runbooks/r.md".into(), 5, "method".into(), "device:stroke".into()),
        ("docs/runbooks/r.md".into(), 6, "method".into(), "device:bogus".into()),
        ("docs/solutions/a.md".into(), 1, "path".into(), "clis/gone/src/x.ts".into()),
        ("docs/solutions/a.md".into(), 1, "package".into(), "@lib/gone".into()),
        ("docs/solutions/a.md".into(), 2, "link".into(), "c.md".into()),
        ("docs/solutions/a.md".into(), 2, "script".into(), "pnpm run gone".into()),
        ("docs/solutions/a.md".into(), 5, "command".into(), "wf secrets".into()),
        ("scripts/run.sh".into(), 3, "command".into(), "wf secrets".into()),
    ]);
    assert!(v["next"].as_str().is_some_and(|n| !n.is_empty()), "{v}");

    // When every reference points at what exists, Then the check passes
    put(root, "docs/solutions/a.md", "See `clis/real/src/lib.rs`; import `@lib/real`.\nRun `pnpm run real`; read [b](b.md).\n```bash\nwf secret get KEY\n```\n");
    put(root, "scripts/run.sh", "#!/bin/sh\nwf secret get KEY\n");
    put(root, "docs/runbooks/r.md", "Tap with `wf android tap-on --point 1,2` or `wf browser tap-on Go`.\n```lua\ndevice:tap_on('Go')\ndevice:get_element({id='Go'})\n```\n");
    put(root, "docs/goals/g.md", "```bash\nwf android test --api | grep -q 'device:assert_screenshot'\n```\n`clis/gone/src/x.ts`\n");
    let r = check();
    assert_eq!((r.exit_code, r.stdout.trim()), (0, r#"{"ok":true,"findings":[],"next":null}"#), "{}", r.stderr);

    // Given the fixed tree staged, and then a dead reference written but not staged
    git(root, &["add", "-A"]);
    put(root, "docs/solutions/a.md", "See `clis/gone/src/x.ts`.\n");
    let staged = || run_wf(&["check", "--dead-refs", "--staged"], &RunOpts::default().cwd(root));
    // When the working tree and the index are checked, Then only the working tree has it
    assert_eq!((check().exit_code, staged().exit_code), (1, 0));
    // When it is staged, Then the index has it too
    git(root, &["add", "-A"]);
    let r = staged();
    assert!(r.exit_code == 1 && r.stdout.contains("clis/gone/src/x.ts"), "{}", r.stdout);
}
