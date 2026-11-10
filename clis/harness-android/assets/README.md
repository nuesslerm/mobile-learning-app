# assets

`u2.jar` is the on-device UI server of [openatx/uiautomator2](https://github.com/openatx/uiautomator2) 3.2.9 (MIT licence),
taken unmodified from the `uiautomator2==3.2.9` wheel on PyPI (`uiautomator2/assets/u2.jar`). `wf android` pushes it to
`/data/local/tmp` and runs it with `app_process` so find/wait/text queries take milliseconds instead of a 3 to 12 s
`uiautomator dump` (see `src/android_u2.rs`). Size 3.7 MB, sha256 `1bd7c57fe019d0275a66b936ed5d7ea5f244d98e5e3ba3d1ccf4a14e721ef94f`.

Vetting: maintained (releases through 2025), widely used, MIT, no network access of its own (it listens on the device's
loopback port 9008 and is reached through `adb forward`). To refresh: download the wheel for the new version, replace the jar,
update this file and the sha, run `cargo test -p wf-harness --test flow android::a_runbook_drives_the_screen_through_the_on_device_ui_server`.
