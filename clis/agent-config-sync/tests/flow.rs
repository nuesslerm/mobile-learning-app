//! Flows through the command line (`cli::run_in`: `[sync] [--check]` and `mcp-guard`) on a throwaway checkout.
#[path = "flow/checkout.rs"]
mod checkout;

use checkout::{golden, golden_paths, here, Checkout};
use serde_json::Value;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn given_the_fixture_when_it_is_synced_checked_edited_and_tampered_with_then_every_output_matches_the_golden_tree_and_drift_is_named_and_repaired() {
    // Given the fixture checkout
    let c = Checkout::new(&[]);

    // When it is synced the first time
    let first = c.run(&["sync"]);
    // Then every generated file is listed and matches the TypeScript output byte for byte, setup.sh executable
    assert_eq!(first.code, 0, "{}", first.stderr);
    let mut synced: Vec<String> = first.stdout.lines().map(|l| l.trim_start_matches("synced: ").to_string()).collect();
    synced.sort();
    assert_eq!(synced, golden_paths());
    for path in golden_paths() {
        assert_eq!(c.read(&path), golden(&path, &c.root), "{path}");
    }
    assert_eq!(fs::metadata(c.path(".claude/setup.sh")).unwrap().permissions().mode() & 0o111, 0o111);

    // When it is checked in every spelling, with wf's own options wherever they stand, and synced again
    // Then each check exits 0 silently and the second sync changes nothing
    for args in [&["--check"][..], &["sync", "--check"], &["sync", "--check", "--app=other"], &["--json", "--check", "--app", "x"]] {
        let check = c.run(args);
        assert_eq!((check.code, check.stdout.as_str(), check.stderr.as_str()), (0, "", ""), "{args:?}");
    }
    for args in [&[][..], &["--app", "x", "sync", "--json"]] {
        let again = c.run(args);
        assert_eq!((again.code, again.stdout.as_str()), (0, ""), "{args:?}");
    }

    // When an instruction source is edited
    c.write(".agents/instructions/root.md", "# Changed\n");
    let check = c.run(&["--check"]);
    // Then the check names both generated files and exits 1, and a sync repairs them
    assert_eq!(check.code, 1);
    assert_eq!(check.stderr, "drift: AGENTS.md (run `pnpm run sync:agent-config`)\ndrift: CLAUDE.md (run `pnpm run sync:agent-config`)\n");
    c.run(&["sync"]);
    assert_eq!(c.run(&["--check"]).code, 0);

    // When a stale file lands in an owned directory and a legacy directory appears
    c.write(".claude/commands/old.md", "stale\n");
    c.write(".gemini/x.toml", "legacy\n");
    let out = c.run(&["sync"]);
    // Then sync names and removes both
    assert!(out.stdout.contains("synced: .gemini"), "{}", out.stdout);
    assert!(out.stdout.contains("synced: .claude/commands/old.md"), "{}", out.stdout);
    assert!(!c.path(".claude/commands/old.md").exists());
    assert!(!c.path(".gemini").exists());

    // When a generated file is replaced by a symlink to its source
    let target = c.path(".claude/commands/hello.md");
    fs::remove_file(&target).unwrap();
    symlink(c.path(".agents/commands/hello.md"), &target).unwrap();
    let check = c.run(&["--check"]);
    c.run(&["sync"]);
    // Then the check fails and sync puts a real file back
    assert_eq!(check.code, 1);
    assert!(!fs::symlink_metadata(&target).unwrap().file_type().is_symlink());
    assert_eq!(c.run(&["--check"]).code, 0);

    // When the command line is something else
    let bogus = c.run(&["sync", "--bogus"]);
    // Then clap's error is shown with exit 2 and nothing is written
    assert_eq!(bogus.code, 2);
    assert!(bogus.stderr.contains("unexpected argument '--bogus' found"), "{}", bogus.stderr);
    assert_eq!(c.run(&["nope"]).code, 2);
    assert_eq!(c.run(&["--app"]).code, 2);
}

#[test]
fn given_settings_claude_code_wrote_or_a_broken_command_when_synced_then_foreign_keys_survive_and_the_broken_source_fails_loud() {
    // Given settings.json carrying keys Claude Code wrote itself beside managed ones
    let c = Checkout::new(&[(".claude/settings.json", "{\"enabledPlugins\":{\"a\":true},\"outputStyle\":\"old\",\"permissions\":{\"allow\":[\"x\"]},\"zeta\":1}")]);
    // When it is synced
    assert_eq!(c.run(&["sync"]).code, 0);
    // Then the foreign keys stay in place and the managed keys are regenerated after them
    let v: Value = serde_json::from_str(&c.read(".claude/settings.json")).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["enabledPlugins", "zeta", "outputStyle", "permissions", "hooks"]);
    assert_eq!(v["outputStyle"], "terse");

    // Given a command whose frontmatter has no description
    let broken = Checkout::new(&[(".agents/commands/hello.md", "---\nname: x\n---\nBody\n")]);
    // When it is synced
    let out = broken.run(&["sync"]);
    // Then it exits 1, loud, naming the file
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("NOT ENFORCING"), "{}", out.stderr);
    assert!(out.stderr.contains(".agents/commands/hello.md: frontmatter needs a description"), "{}", out.stderr);
}

#[test]
fn given_an_env_map_in_the_claude_settings_source_when_synced_then_it_lands_in_settings_and_a_non_string_value_fails_loud() {
    // Given a source that blanks one variable and sets another
    let c = Checkout::new(&[(".agents/config/claude-settings.json", "{\"env\":{\"BLANKED\":\"\",\"SET\":\"1\"}}")]);
    // When it is synced
    assert_eq!(c.run(&["sync"]).code, 0);
    // Then both are in settings.json, the blank one as an empty string
    let v: Value = serde_json::from_str(&c.read(".claude/settings.json")).unwrap();
    assert_eq!(v["env"], serde_json::json!({ "BLANKED": "", "SET": "1" }));

    // Given a source whose variable is not a string
    let broken = Checkout::new(&[(".agents/config/claude-settings.json", "{\"env\":{\"N\":1}}")]);
    // When it is synced, Then it exits 1 naming the variable
    let out = broken.run(&["sync"]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("env.N must be a string"), "{}", out.stderr);
}

#[test]
fn given_the_mcp_guard_when_tools_of_each_tier_are_called_then_the_decision_matches_the_policy_and_a_broken_contract_fails_closed() {
    // Given the fixture, whose mcps.json declares `demo`
    let c = Checkout::new(&[]);
    let allow = "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\"}}\n";

    // When a call of each tier reaches the hook
    // Then declared and first-party servers and non-MCP tools pass through, a remote session's UUID server and a
    // read-shaped tool are allowed, and a write-shaped tool is denied byte-identically to the TypeScript guard
    assert_eq!(c.guard("mcp__demo__anything_at_all"), (0, String::new()));
    for first_party in ["github__create_pull_request", "Claude_Code_Remote__create_session", "ccd_session__spawn_task", "Claude_Docs__batch"] {
        assert_eq!(c.guard(&format!("mcp__{first_party}")), (0, String::new()), "{first_party}");
    }
    assert_eq!(c.guard("Bash"), (0, String::new()));
    assert_eq!(c.guard("mcp__bf7c680d-5fdc-5ef4-b4a0-abadb619bf0a__create_session"), (0, allow.to_string()));
    assert_eq!(c.guard("mcp__Notion__notion-fetch"), (0, allow.to_string()));
    assert_eq!(c.guard("mcp__Slack__slack_send_message"), (0, fs::read_to_string(here().join("golden-guard-deny.json")).unwrap()));
    let with_wf_options = c.run_with_stdin(&["mcp-guard", "--json", "--app", "x"], "{\"tool_name\":\"mcp__Notion__notion-fetch\"}");
    assert_eq!((with_wf_options.code, with_wf_options.stdout.as_str()), (0, allow));

    // Given a broken mcps.json
    let broken = Checkout::new(&[(".agents/config/mcps.json", "{")]);
    // When the guard runs
    let out = broken.run_with_stdin(&["mcp-guard"], "{\"tool_name\":\"mcp__Slack__slack_send_message\"}");
    // Then it fails loud with exit 1 and no decision, so the call is never read as allowed
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    assert!(out.stderr.contains("NOT ENFORCING"), "{}", out.stderr);
}

#[test]
fn an_opencode_bash_pattern_map_reaches_opencode_json_and_a_pattern_that_asks_is_refused() {
    // Given the fixture with an opencode bash map that denies resetting a shared checkout
    let fixture: Value = serde_json::from_str(&fs::read_to_string(here().join("fixture.json")).unwrap()).unwrap();
    let mut perms: Value = serde_json::from_str(fixture[".agents/config/permissions.json"].as_str().unwrap()).unwrap();
    perms["opencode"]["permission"] = serde_json::json!({ "bash": { "*": "allow", "git stash*": "deny" } });
    let c = Checkout::new(&[(".agents/config/permissions.json", &perms.to_string())]);

    // When it is synced
    let synced = c.run(&["sync"]);
    // Then opencode.json carries the map as written
    assert_eq!(synced.code, 0, "{}", synced.stderr);
    let opencode: Value = serde_json::from_str(&c.read("opencode.json")).unwrap();
    assert_eq!(opencode["permission"]["bash"], serde_json::json!({ "*": "allow", "git stash*": "deny" }));

    // When one pattern asks instead
    perms["opencode"]["permission"]["bash"]["git push*"] = serde_json::json!("ask");
    c.write(".agents/config/permissions.json", &perms.to_string());
    let refused = c.run(&["sync"]);
    // Then the sync fails naming the tool and the pattern
    assert_ne!(refused.code, 0);
    assert!(refused.stderr.contains("opencode permission \"bash\" pattern \"git push*\" must be allow or deny"), "{}", refused.stderr);
}
