// The features the app lints: in-app features are cruised in place, feature packages lint themselves and the app's oxlint skips them.
// The region below is generated from each feature's `arch` key by `wf check --layout --write`; a new feature directory needs no script edit.
// BEGIN generated:feature-map
const FEATURES = {
  dashboards: { type: 'hexagonal-feature' },
  leetcode: { type: 'hexagonal-feature', subFeatures: ['plan'] },
  'tamil-writing': { type: 'hexagonal-feature' },
};
const PACKAGES = ['coding-agent', 'service-report', 'settings'];
// END generated:feature-map

const cruisePaths = Object.keys(FEATURES).map((name) => `features/${name}`);
const oxlintIgnores = PACKAGES.map((name) => `--ignore-pattern=features/${name}/**`);

module.exports = { FEATURES, PACKAGES, cruisePaths, oxlintIgnores };

if (require.main === module) {
  const lists = { cruise: cruisePaths, 'oxlint-ignore': oxlintIgnores };
  process.stdout.write(`${(lists[process.argv[2]] ?? []).join(' ')}\n`);
}
