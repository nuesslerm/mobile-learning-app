import { LoggerlessTransport } from '@loglayer/transport';
import { LogLayer } from 'loglayer';
import { describe, it, expect, beforeEach } from 'vitest';

import {
  Logger,
  configureLogger,
  __resetLoggerConfig,
} from '../src/logger/Logger.js';
import { scrubberPlugin } from '../src/plugins/scrubberPlugin.js';

type Captured = { messages: unknown[]; data?: Record<string, unknown> };

class CaptureTransport extends LoggerlessTransport {
  public entries: Captured[] = [];
  constructor() {
    super({ id: 'capture' });
  }
  shipToLogger(params: {
    messages: unknown[];
    data?: Record<string, unknown>;
  }): unknown[] {
    this.entries.push({ messages: params.messages, data: params.data });
    return params.messages;
  }
}

describe('given a LogLayer with the scrubber plugin attached directly', () => {
  it('when a log has a sensitive-keyed field + PII in the message, then both are redacted before the transport', () => {
    const capture = new CaptureTransport();
    const layer = new LogLayer({ transport: capture });
    layer.addPlugins([scrubberPlugin({ scrubEnvValues: false })]);

    layer
      .withMetadata({ password: 'hunter2', user: 'bob' })
      .info('reset link for jane@example.com');

    const last = capture.entries.at(-1)!;
    expect(last.data).toMatchObject({ password: '[REDACTED]', user: 'bob' });
    const msg = String(last.messages.join(' '));
    expect(msg).toContain('[REDACTED]');
    expect(msg).not.toContain('jane@example.com');
  });
});

describe('given configureLogger with the default scrub setting', () => {
  beforeEach(() => __resetLoggerConfig());

  it('when scrub is left default, then Logger.* output is auto-scrubbed', () => {
    const capture = new CaptureTransport();
    const layer = new LogLayer({ transport: capture });
    configureLogger({ root: () => layer, scrub: { scrubEnvValues: false } });

    Logger.withMetadata({ apiKey: 'super-secret-value' }).info(
      'hi jane@example.com'
    );

    const last = capture.entries.at(-1)!;
    expect(last.data).toMatchObject({ apiKey: '[REDACTED]' });
    expect(String(last.messages.join(' '))).not.toContain('jane@example.com');
  });

  it('when scrub:false, then it opts out (no redaction)', () => {
    const capture = new CaptureTransport();
    const layer = new LogLayer({ transport: capture });
    configureLogger({ root: () => layer, scrub: false });

    Logger.withMetadata({ apiKey: 'super-secret-value' }).info('x');

    expect(capture.entries.at(-1)!.data).toMatchObject({
      apiKey: 'super-secret-value',
    });
  });
});
