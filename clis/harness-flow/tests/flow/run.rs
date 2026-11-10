//! `wf flow run`: vitest runs once per theme with the golden switches in its environment, and a run that froze no screen says so.
use crate::support::SlotSandbox;

const VITEST_STUB: &str = "echo \"$FLOW_GOLDENS $FLOW_THEME $*\" >> \"$0.log\"\nexit 0";

#[test]
fn a_flow_run_starts_vitest_once_per_theme_and_says_when_no_screen_was_frozen() {
    // Given the app and a vitest stub that freezes nothing
    let s = SlotSandbox::new("wf-flow-run-");
    s.file("frontend/mla-tamagui/package.json", "{}");
    s.stub("pnpm", VITEST_STUB);

    // When one flow file runs with --goldens in both themes
    let r = s.run(&["flow", "run", "features/editor/Editor.flow.test.tsx", "--goldens"], &[]);

    // Then vitest ran for that file once per theme with the goldens switched on
    let calls = s.calls("pnpm");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].starts_with("1 light exec vitest run") && calls[1].starts_with("1 dark exec vitest run"), "{calls:?}");
    assert!(calls.iter().all(|c| c.ends_with("features/editor/Editor.flow.test.tsx")), "{calls:?}");
    // and with no frozen screen the golden pass fails and tells how to get one
    let out = format!("{}{}", r.stdout, r.stderr);
    assert_eq!(r.exit_code, 1, "{out}");
    assert!(out.contains("run the flow tests with FLOW_GOLDENS=1 first"), "{out}");
}

#[test]
fn flow_goldens_without_a_snapshot_directory_names_the_next_step() {
    // Given a directory with no snapshots
    let s = SlotSandbox::new("wf-flow-goldens-none-");

    // When the golden pass is asked for them
    let r = s.run(&["flow", "goldens", "--snapshots", "missing", "--goldens", "goldens"], &[]);

    // Then it fails before it starts a browser and says what to run
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
    assert!(r.stdout.contains("\"ok\":false") && r.stdout.contains("FLOW_GOLDENS=1"), "{}", r.stdout);
}
