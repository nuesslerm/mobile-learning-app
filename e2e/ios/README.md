# Exercise screenshot regression tests

Use an iPhone 18 Pro Max simulator (the reference device is named `rca`) on iOS 27.0 in portrait, English (US), with Expo Go 58 and the software keyboard enabled. Open the MLA project in Expo Go once and dismiss the developer-menu introduction. Start with the default theme preset, the JavaScript starter for Two Sum and the on-device runner. The incomplete starter produces a deterministic Wrong Answer for the first example.

Run from the repository root with the Python interpreter containing fb-idb. Supply the Expo project URL ending in `/--`, the simulator UDID, and the installed `wf` binary.

```sh
for appearance in light dark; do
  "$IDB_PYTHON" e2e/ios/exercise_goldens.py \
    --device "$EXERCISE_UDID" --base "$EXERCISE_EXPO_BASE" \
    --appearance "$appearance" --wf "$WF_BIN" || exit 1
done
```

The test terminates Expo Go, saves this project's previous Tamil course progress in a temporary directory, and relaunches the app. It selects the requested appearance through Settings. It follows the bundled lessons, traces the first vowel using its authored guide, and opens the Two Sum editor from its library row. The comparison includes the keyboard, feedback and drawing surface. Each state goes through `wf test ios-visual`. Use `--update-goldens` only to record a reviewed visual change.

The RCA simulator baseline uses a clean, isolated build of dev revision `8b230d5ed206794d74bda422a8eaecc13279eb51`. The app source had an empty working-tree diff against that revision. Its minified iOS Hermes bundle had SHA-256 `75409fd948c0651f8333cc277d78f4208fda6382c83a616c0ed5e714818a889f`. A second capture matched all 22 references with zero differing pixels. Substituting the listen prompt for the fill prompt failed the comparator with 53,372 differing pixels. The existing native screenshot harness writes the comparison result and pixel diff on failure.

For installations of fb-idb that need a separate input companion, pass its Unix socket using `--companion`. The `wf` comparison still addresses the simulator by UDID.
