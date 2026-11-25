# Integrated home tabs on the RCA iPhone

Both installed apps ran on the iOS 27.0 simulator named `rca` in light mode, with an empty host catalog, the keyboard closed, and identical display and font settings. The Orca binary uses the pinned reference commit. Expo opened the coding-agent entry with `presentation=orca`. The captures show the settled state after each selection. The earlier unpaired recording remains the transition reference.

| Tab | Orca | Expo | Differing pixels |
| --- | --- | --- | ---: |
| Feed | [Reference](orca/feed.png) | [Candidate](expo/feed.png) | 0 |
| Agents | [Reference](orca/Agents.png) | [Candidate](expo/Agents.png) | 0 |
| Usage | [Reference](orca/Usage.png) | [Candidate](expo/Usage.png) | 0 |
| Automations | [Reference](orca/Automations.png) | [Candidate](expo/Automations.png) | 0 |
| Settings | [Reference](orca/Settings.png) | [Candidate](expo/Settings.png) | 14,975 |

Each comparison covers 3,762,045 pixels. The two masks cover the iOS clock and the previous-app label. Settings retains differences in the two app update rows. Connected states, other themes, keyboard-open states and transitions require their own comparisons.

With either app on its unpaired home screen, run the [tab journey](tabs.lua) against it. Pass a separate absolute output directory for each app and use the same simulator. Compare the resulting images from the repository root.

```sh
wf ios test tabs.lua --device "$RCA_UDID" --env out="$CAPTURE_DIR" --test-output-dir "$RECORDING_DIR"
wf test ios-visual home-agents --exact --max-diff-ratio 0 \
  --reference "$ORCA_DIR/Agents.png" --capture "$EXPO_DIR/Agents.png" \
  --ignore 205,50,340,120 --ignore 30,113,420,152
```
