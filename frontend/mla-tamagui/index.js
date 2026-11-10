// Root entry point for both iOS native bundler and web export.
//
// package.json's `main` field points here; this file re-exports
// `expo-router/entry` so file-based routing under ./app is used.
// SDK 54 ignores the legacy `expo.entryPoint` config field, so the entry
// must be declared via `main`. Using a local `index.js` (instead of setting
// `main` directly to `expo-router/entry`) keeps monorepo / pnpm resolution
// deterministic and gives us a place for future side-effect setup.
import 'expo-router/entry';
