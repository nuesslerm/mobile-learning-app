function trackAnalytics() {
  // No `Logger` import anywhere in this module — some other global/ambient
  // `Logger` (or a typo/coincidental name) must never be rewritten.
  Logger.info('not our logger');
}
