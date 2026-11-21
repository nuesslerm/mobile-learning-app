import { describe, expect, it } from 'vitest';

import { deriveCommentsUrl } from '../deriveCommentsUrl';

describe('given a health_reports ORDS base URL', () => {
  it('when deriveCommentsUrl runs, then it swaps the trailing path segment for golden_screenshot_comments', () => {
    expect(
      deriveCommentsUrl('https://example-ords.test/ords/health/health_reports/')
    ).toBe('https://example-ords.test/ords/health/golden_screenshot_comments/');
  });
});
