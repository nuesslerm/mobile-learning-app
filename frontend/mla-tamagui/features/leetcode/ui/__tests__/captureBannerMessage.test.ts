import { describe, it, expect } from 'vitest';
import { deriveCaptureBannerMessage } from '../captureBannerMessage';

describe('deriveCaptureBannerMessage', () => {
  it('given inactive capture when called then returns null', () => {
    const result = deriveCaptureBannerMessage({ active: false, expiresAt: null }, 1_000);
    expect(result).toBeNull();
  });

  it('given active capture when called then returns countdown message', () => {
    const result = deriveCaptureBannerMessage({ active: true, expiresAt: 61_000 }, 1_000);
    expect(result).toBe('Login-capture in progress — expires in 1m.');
  });

  it('given short remaining time when calculated then rounds down to whole minutes with floor at 0', () => {
    const result = deriveCaptureBannerMessage({ active: true, expiresAt: 500 }, 1_000);
    expect(result).toBe('Login-capture in progress — expires in 0m.');
  });
});
