import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import { ALLOWED_ROUTES } from './devRuntimeConfig';

vi.mock('./nativeLaunchArgs', () => ({
  LaunchArguments: { value: vi.fn() },
}));

describe('dev-runtime-config.schema.json stays in sync with ALLOWED_ROUTES', () => {
  it('JSON Schema enum matches the ALLOWED_ROUTES source of truth (plus null)', () => {
    const schemaPath = resolve(__dirname, 'dev-runtime-config.schema.json');
    // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
    const schema = JSON.parse(readFileSync(schemaPath, 'utf8')) as {
      properties: { initial_route: { enum: (string | null)[] } };
    };
    const enumInSchema = schema.properties.initial_route.enum;
    // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
    const allowedSet = new Set<string>(ALLOWED_ROUTES as readonly string[]);
    const expectedEnum = [null, ...ALLOWED_ROUTES];
    expect(enumInSchema.sort()).toEqual([...expectedEnum].sort());
    for (const value of enumInSchema) {
      if (value === null) continue;
      expect(allowedSet.has(value)).toBe(true);
    }
  });
});
