import { LoggerlessTransport } from '@loglayer/transport';
import { LogLayer } from 'loglayer';
import { describe, expect, it, beforeEach } from 'vitest';

import {
  Logger,
  __resetLoggerConfig,
  configureLogger,
  isLoggerConfigured,
} from '../src/logger/Logger';

type Captured = { messages: unknown[]; data?: Record<string, unknown> };

class CaptureTransport extends LoggerlessTransport {
  public entries: Captured[] = [];
  constructor() {
    super({ id: 'capture' });
  }
  shipToLogger(params: { messages: unknown[]; data?: Record<string, unknown> }): unknown[] {
    this.entries.push({ messages: params.messages, data: params.data });
    return params.messages;
  }
}

describe('Logger static API', () => {
  describe('given configureLogger() has not been called', () => {
    beforeEach(() => {
      __resetLoggerConfig();
    });

    it('when a static level method is called, then it is a no-op and does not throw', () => {
      expect(() => Logger.info('before.configure')).not.toThrow();
      expect(() => Logger.error('before.configure')).not.toThrow();
    });

    it('when isLoggerConfigured() is checked, then it is false', () => {
      expect(isLoggerConfigured()).toBe(false);
    });
  });

  describe('given configureLogger() has been called', () => {
    beforeEach(() => {
      __resetLoggerConfig();
      configureLogger({ root: () => null });
    });

    it('when isLoggerConfigured() is checked, then it is true', () => {
      expect(isLoggerConfigured()).toBe(true);
    });
  });
});

describe('given a Logger instance with context', () => {
  beforeEach(() => __resetLoggerConfig());

  it('when it logs twice, then each line carries the context', () => {
    const capture = new CaptureTransport();
    const layer = new LogLayer({ transport: capture });
    configureLogger({ root: () => layer, scrub: false });

    const log = Logger.withContext({ file: 'features/x/ui/Screen.tsx', line: 12 });
    log.info('first');
    log.info('second', { id: 1 });

    expect(capture.entries).toHaveLength(2);
    expect(capture.entries[0]?.data).toMatchObject({ file: 'features/x/ui/Screen.tsx', line: 12 });
    expect(capture.entries[1]?.data).toMatchObject({ file: 'features/x/ui/Screen.tsx', line: 12, id: 1 });
  });
});
