//! The rules that keep the fixed cost before the first test down: what a run does not read, it does not fetch.
use crate::support::repo_root;

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path} is readable: {e}"))
}

#[test]
fn a_core_or_cli_run_plans_no_legs_in_a_trivial_step_and_the_other_stages_still_ask_the_vault() {
    // Given the pipeline's plan job
    let text = read(".github/workflows/pipeline.yml");

    // When the step that writes the legs and the step that asks for them are read
    let none = text.find("id: nolegs").expect("a step writes the empty legs");
    let none_block = &text[none..none + 400];
    let ask = text.find("id: legs\n").expect("a step asks which services changed");
    let ask_block = &text[ask..ask + 200];

    // Then the empty step runs for core and cli only, writes the values the deploy and e2e jobs test for, and the real one runs for every other stage
    assert!(none_block.contains("inputs.stages == 'core' || inputs.stages == 'cli'"), "{none_block}");
    assert!(none_block.contains("legs=[]") && none_block.contains("eas_update=false"), "{none_block}");
    assert!(ask_block.contains("inputs.stages != 'core' && inputs.stages != 'cli'"), "{ask_block}");
    assert!(text.contains("legs: ${{ steps.legs.outputs.legs || steps.nolegs.outputs.legs }}"));
    assert!(text.contains("eas_update: ${{ steps.legs.outputs.eas_update || steps.nolegs.outputs.eas_update }}"));
}

#[test]
fn chromium_and_its_system_packages_come_from_a_cache_keyed_by_version_and_image_with_the_mirror_as_the_fallback() {
    // Given the install-test-deps action
    let text = read(".github/actions/install-test-deps/action.yml");

    // When its Playwright steps are read
    let key = text.find("id: pw-key").expect("a step keys the cache");
    let install = text.find("name: Chromium for Playwright").expect("the install step");
    let save = text.find("name: Save the Playwright cache").expect("the save step");

    // Then the key names the Playwright version and the runner image, the restore comes before the install, the .deb files install with dpkg before any apt
    // call, the mirror path stays behind them, and the saved tree carries no marker
    assert!(key < install && install < save);
    assert!(text.contains("playwright-$version-$ImageVersion-"));
    assert!(text.contains("[ -n \"${ImageVersion:-}\" ] || exit 0"));
    assert!(text.contains("dpkg -i \"$debs\"/*.deb"));
    assert!(text.find("dpkg -i \"$debs\"").unwrap() < text.find("install --with-deps chromium").unwrap());
    assert!(text.contains("rm -f \"$HOME\"/.cache/ms-playwright/.installed-*"));
    assert!(text.contains("steps.playwright.outcome == 'success'"));
    assert!(text.contains("steps.pw-prep.outputs.save == 'true'"));
}
