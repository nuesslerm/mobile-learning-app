import { describe, expect, it } from 'vitest';

import { resolveInitialRoute } from './resolveInitialRoute';

describe('resolveInitialRoute precedence', () => {
  it('returns file over envVar and default (development)', () => {
    expect(
      resolveInitialRoute({
        file: '/from-file',
        envVar: '/from-env',
        perEnvDefault: '/from-default',
        isProduction: false,
      })
    ).toBe('/from-file');
  });

  it('returns envVar when file is absent', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: '/from-env',
        perEnvDefault: '/from-default',
        isProduction: false,
      })
    ).toBe('/from-env');
  });

  it('returns perEnvDefault when only the default is set', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: undefined,
        perEnvDefault: '/from-default',
        isProduction: false,
      })
    ).toBe('/from-default');
  });

  it('returns undefined when all sources are empty', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: undefined,
        perEnvDefault: undefined,
        isProduction: false,
      })
    ).toBeUndefined();
  });

  it('SKIPS the file source when isProduction is true', () => {
    expect(
      resolveInitialRoute({
        file: '/from-file',
        envVar: undefined,
        perEnvDefault: '/from-default',
        isProduction: true,
      })
    ).toBe('/from-default');
  });

  it('envVar wins over file in production (file skipped, envVar used)', () => {
    expect(
      resolveInitialRoute({
        file: '/from-file',
        envVar: '/from-env',
        perEnvDefault: '/from-default',
        isProduction: true,
      })
    ).toBe('/from-env');
  });

  it('returns perEnvDefault in production when only perEnvDefault is set', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: undefined,
        perEnvDefault: '/from-default',
        isProduction: true,
      })
    ).toBe('/from-default');
  });

  it('returns undefined in production when only file is set (skipped, no other sources)', () => {
    expect(
      resolveInitialRoute({
        file: '/from-file',
        envVar: undefined,
        perEnvDefault: undefined,
        isProduction: true,
      })
    ).toBeUndefined();
  });

  it('empty envVar falls through to perEnvDefault', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: '',
        perEnvDefault: '/from-default',
        isProduction: false,
      })
    ).toBe('/from-default');
  });

  it('empty perEnvDefault returns undefined', () => {
    expect(
      resolveInitialRoute({
        file: null,
        envVar: undefined,
        perEnvDefault: '',
        isProduction: false,
      })
    ).toBeUndefined();
  });
});
