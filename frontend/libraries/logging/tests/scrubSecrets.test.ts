import { describe, it, expect, afterEach } from 'vitest';

import {
  scrubSecrets,
  createScrubber,
} from '../src/primitives/secretRedaction.js';

describe('given scrubSecrets', () => {
  describe('when a field key looks sensitive', () => {
    it('then it redacts the value (case-insensitive, substring match on env-var-style keys)', () => {
      const out = scrubSecrets({
        BWS_ACCESS_TOKEN: 'abc',
        OP_SERVICE_ACCOUNT_TOKEN: 'def',
        password: 'hunter2',
        apiKey: 'k',
        note: 'keep me',
      });
      expect(out).toEqual({
        BWS_ACCESS_TOKEN: '[REDACTED]',
        OP_SERVICE_ACCOUNT_TOKEN: '[REDACTED]',
        password: '[REDACTED]',
        apiKey: '[REDACTED]',
        note: 'keep me',
      });
    });
  });

  describe('when a field key is not sensitive', () => {
    it('then the value passes through untouched (including a `key` field holding a secret NAME)', () => {
      const out = scrubSecrets({ key: 'TELEGRAM_BOT_TOKEN', count: 3, ok: true });
      expect(out).toEqual({ key: 'TELEGRAM_BOT_TOKEN', count: 3, ok: true });
    });
  });

  describe('when the value is nested', () => {
    it('then it recurses through objects and arrays', () => {
      const out = scrubSecrets({
        outer: { token: 's1', items: [{ secret: 's2', name: 'n' }] },
      });
      expect(out).toEqual({
        outer: {
          token: '[REDACTED]',
          items: [{ secret: '[REDACTED]', name: 'n' }],
        },
      });
    });
  });

  describe('when given the input', () => {
    it('then it never mutates the original', () => {
      const input = { token: 'x', nested: { password: 'y' } };
      scrubSecrets(input);
      expect(input).toEqual({ token: 'x', nested: { password: 'y' } });
    });
  });

  describe('when custom patterns / replacement are supplied', () => {
    it('then it honors them', () => {
      const out = scrubSecrets(
        { pin: '1234', other: 'z' },
        { patterns: ['pin'], replacement: '***' }
      );
      expect(out).toEqual({ pin: '***', other: 'z' });
    });
  });
});

describe('given createScrubber (comprehensive)', () => {
  const saved = { ...process.env };
  afterEach(() => {
    for (const k of Object.keys(process.env))
      if (!(k in saved)) delete process.env[k];
    Object.assign(process.env, saved);
  });

  describe('when a sensitive-named env var value appears in a message string', () => {
    it('then it is redacted wherever it appears (auto-scrub env mentions)', () => {
      process.env['MY_API_TOKEN'] = 'sk-live-abcdef123456';
      const scrub = createScrubber();
      const out = scrub({
        message: 'calling api with sk-live-abcdef123456 now',
        ok: true,
      });
      expect(out).toEqual({
        message: 'calling api with [REDACTED] now',
        ok: true,
      });
    });
  });

  describe('when an env var value is short', () => {
    it('then it is NOT used for redaction (guards against nuking common strings)', () => {
      process.env['SHORT_SECRET'] = 'ab'; // below the default min length
      const scrub = createScrubber();
      expect(scrub({ note: 'ab cd ab' })).toEqual({ note: 'ab cd ab' });
    });
  });

  describe('when a string contains PII', () => {
    it('then emails, JWTs, and card-like runs are redacted', () => {
      const scrub = createScrubber({ scrubEnvValues: false });
      expect(scrub('contact me at jane.doe@example.com')).toBe(
        'contact me at [REDACTED]'
      );
      expect(scrub('token eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM')).toBe(
        'token [REDACTED]'
      );
      expect(scrub('card 4242 4242 4242 4242 end')).toBe('card [REDACTED] end');
    });
  });

  describe('when PII scrubbing is disabled', () => {
    it('then emails pass through', () => {
      const scrub = createScrubber({ scrubEnvValues: false, scrubPii: false });
      expect(scrub('jane@example.com')).toBe('jane@example.com');
    });
  });

  describe('when an extra literal secret value is supplied', () => {
    it('then it is redacted anywhere it appears', () => {
      const scrub = createScrubber({
        scrubEnvValues: false,
        extraSecretValues: ['hunter2xyz'],
      });
      expect(scrub({ msg: 'pw=hunter2xyz' })).toEqual({ msg: 'pw=[REDACTED]' });
    });
  });
});
