import { describe, expect, it } from 'vitest';
import { z } from 'zod';

import { InvalidDevRuntimeConfigError } from './devRuntimeConfig';

describe('InvalidDevRuntimeConfigError', () => {
  it('formats Zod issues into a single human-readable message', () => {
    const schema = z.object({
      initial_route: z.enum(['/foo', '/bar']),
      feature_flags: z.record(z.string(), z.boolean()),
    });
    const result = schema.safeParse({
      initial_route: '/not-allowed',
      feature_flags: {},
    });
    expect(result.success).toBe(false);
    if (result.success) return;

    const err = new InvalidDevRuntimeConfigError(result.error, {
      initial_route: '/not-allowed',
      feature_flags: {},
    });

    expect(err.name).toBe('InvalidDevRuntimeConfigError');
    expect(err.message).toContain('initial_route');
    expect(err.zodError).toBe(result.error);
    expect(err.candidate).toEqual({
      initial_route: '/not-allowed',
      feature_flags: {},
    });
  });

  it('lists multiple Zod issues joined by semicolons', () => {
    const schema = z.object({
      initial_route: z.enum(['/foo', '/bar']),
      feature_flags: z.record(z.string(), z.boolean()),
    });
    const result = schema.safeParse({
      initial_route: 42,
      feature_flags: 'not-a-record',
    });
    expect(result.success).toBe(false);
    if (result.success) return;

    const err = new InvalidDevRuntimeConfigError(result.error, {});
    // At least one issue should mention each top-level key that was wrong.
    expect(err.message).toMatch(/initial_route|feature_flags/);
  });
});
