import { describe, expect, it } from 'vitest';

import { encodeBase64 } from '../base64';

describe('given a plain user:password string', () => {
  it('when encodeBase64 runs, then it matches the standard base64 encoding', () => {
    expect(encodeBase64('health_rpt:s3cret')).toBe(
      Buffer.from('health_rpt:s3cret', 'utf8').toString('base64')
    );
  });

  it('when the input length is not a multiple of 3, then padding is applied correctly', () => {
    expect(encodeBase64('a')).toBe('YQ==');
    expect(encodeBase64('ab')).toBe('YWI=');
    expect(encodeBase64('abc')).toBe('YWJj');
  });

  it('when the input is empty, then the output is empty', () => {
    expect(encodeBase64('')).toBe('');
  });
});
