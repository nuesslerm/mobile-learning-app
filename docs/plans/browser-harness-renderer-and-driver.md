# Browser harness renderer and driver

The renderer already lives in a separate package. `DeviceFrame.tsx` renders the phone bezel and the outer iframe that hosts the app. `App.tsx` owns the shell state and exposes it through the page bridge. `MockKeyboard.tsx` draws the keyboard. On the Rust side, `driver.rs` currently both queries shell geometry and drives the app. Its document walker enters same-origin iframes *inside* the app. A driver that can operate a standalone page still needs an adapter to enter the outer phone iframe and account for the keyboard.

## Proposed structure

```text
tooling/browser-harness/ui/src/
├── App.tsx                 public shell state and app iframe reference
├── DeviceFrame.tsx         bezel, app viewport and outer iframe
└── MockKeyboard.tsx        visible keyboard and its geometry

clis/harness-browser/src/
├── browser_launch.rs       compose the renderer adapter and driver in one session
├── frame/                  adapt the shell bridge to the driver's viewport contract
│   └── mod.rs              app frame, viewport origin, scale and occluded bounds
└── driving/                execute input and assertions against an app document
    ├── mod.rs              selector, visibility and input operations
    └── browser_find.js     find controls across supported nested app frames
```

The driving code consumes an app-document handle and a viewport in CSS coordinates. The frame adapter maps them to the outer page and reports the keyboard's covered rectangle. A plain page supplies its own document and viewport through the same contract. The composition root chooses the adapter once per launch. The Lua journey uses the same commands with either renderer.

A migration should exercise the same control in a plain page, in the phone frame and inside a nested same-origin app iframe. Verify taps after scrolling and with the keyboard open. The current web benchmark exercised the two existing driver configurations. Maestro drove the standalone export. Support for the custom outer frame would require a separate parity run.
