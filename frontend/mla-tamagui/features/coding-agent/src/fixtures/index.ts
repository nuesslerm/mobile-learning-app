// features/coding-agent/src/fixtures/index.ts
//
// Barrel for the deterministic fixtures consumed by the impl, the mocks
// package, and the storybook proposals. Pure data, no React, no IO.
export { FIXTURE_IDS, FIXTURE_THREADS, type FixtureId } from './threads';

export {
  FIXTURE_DISPATCH_RUNS,
  FIXTURE_DISPATCH_RUN_IDS,
} from './dispatchRuns';

export { FIXTURE_SERVER_CONFIG } from './serverConfig';
