//! A flow file's frozen screen painted in a real headless Chromium and turned into a golden by `wf flow run --update-goldens`.
use crate::support::SlotSandbox;

#[test]
fn a_flow_file_with_update_goldens_turns_its_frozen_screen_into_a_golden() {
    assert!(cdp::launch::find_chromium().is_some(), "no Chromium (set WF_CHROMIUM or install one with Playwright)");
    // Given the app with a golden of another suite, and a vitest stub that freezes one screen as the flow tests do
    let s = SlotSandbox::new("wf-flow-goldens-");
    let app = s.repo.join("frontend/mla-tamagui");
    s.file("frontend/mla-tamagui/tests/goldens/other/home.light.png", b"keep");
    s.stub("pnpm", "echo \"$FLOW_GOLDENS $FLOW_THEME $*\" >> \"$0.log\"\nmkdir -p \"$FLOW_GOLDENS_OUT/editor\"\necho '<html><body style=\"background:#fff\"><p>hi</p></body></html>' > \"$FLOW_GOLDENS_OUT/editor/final.html\"\nexit 0");
    // When one flow file runs with --update-goldens in the light theme
    let r = s.run(&["flow", "run", "features/editor/Editor.flow.test.tsx", "--update-goldens", "--theme", "light"], &[]);
    // Then vitest ran once for that file with the goldens switched on
    let calls = s.calls("pnpm");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].starts_with("1 light exec vitest run") && calls[0].ends_with("features/editor/Editor.flow.test.tsx"), "{calls:?}");
    // and the frozen screen became a golden while the other suite's golden stayed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(app.join("tests/goldens/editor/final.light.png").exists() && app.join("tests/goldens/other/home.light.png").exists(), "{}", r.stdout);
    assert!(r.stdout.contains("goldens (light): "), "{}", r.stdout);
}
