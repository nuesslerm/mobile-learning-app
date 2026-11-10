import { __resetLoggerConfig, configureLogger } from '@lib/logging';

export type CapturedLine = {
  level: 'debug' | 'info' | 'warn' | 'error';
  event: string;
  fields: Record<string, unknown>;
};

type LoggerRoot = ReturnType<Parameters<typeof configureLogger>[0]['root']>;

/**
 * The flow setup leaves `Logger` a no-op, so a test that asserts a log line wires this recording root first. Fields are
 * recorded as the code passed them, before the scrubber would run, so a leaked secret shows up here.
 */
export function captureLogs() {
  const lines: CapturedLine[] = [];
  const chain = (fields: Record<string, unknown>): Record<string, unknown> => {
    const record =
      (level: CapturedLine['level']) =>
      (event: string): void => {
        lines.push({ level, event, fields });
      };
    return {
      withContext: () => chain(fields),
      withError: (error: unknown) =>
        chain({
          ...fields,
          error:
            error instanceof Error
              ? { name: error.name, message: error.message }
              : error,
        }),
      withMetadata: (more: Record<string, unknown>) =>
        chain({ ...fields, ...more }),
      debug: record('debug'),
      info: record('info'),
      warn: record('warn'),
      error: record('error'),
    };
  };
  configureLogger({
    root: () => chain({}) as unknown as LoggerRoot,
    scrub: false,
  });
  return {
    lines,
    named: (event: string): CapturedLine[] =>
      lines.filter((line) => line.event === event),
    /** Every captured line as one string, for asserting a value never reached a log. */
    dump: (): string => JSON.stringify(lines),
    stop: (): void => __resetLoggerConfig(),
  };
}
