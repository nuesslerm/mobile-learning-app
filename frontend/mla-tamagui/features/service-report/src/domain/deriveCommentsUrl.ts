/** `.../ords/health/health_reports/` -> `.../ords/health/golden_screenshot_comments/`. */
export function deriveCommentsUrl(healthReportsBaseUrl: string): string {
  return healthReportsBaseUrl.replace(
    /health_reports\/?$/,
    'golden_screenshot_comments/'
  );
}
