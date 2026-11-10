/**
 * Host adapter for `@lib/logging` — the app's ONLY logger wiring.
 *
 * Imported for its side effect, and it must be the first import in
 * `app/_layout.tsx`. `Logger.*` resolves its root lazily and returns a no-op
 * when `configureLogger` has not run, so a missing or late import makes every
 * call in the app AND in the design system silently vanish — an absence that
 * looks exactly like the app being quiet. App #1 carries the same warning.
 *
 *   Logger -> PipelineTransport -> [appendToLiveTail] -> the debug sheet
 *                                -> [appendToBetterStack] -> BetterStack (info and up), when the build carries its ingest pair
 *   console -> installConsoleCapture -> the debug sheet only
 *
 * The forwarder is on only when EXPO_PUBLIC_BETTERSTACK_TOKEN and
 * EXPO_PUBLIC_BETTERSTACK_HOST are both baked in (CI's EAS Update step does it);
 * a local or flow-test run without them posts nothing. No sqlite/idb persistence.
 */
import { Logger, configureLogger } from '@lib/logging';
import { installConsoleCapture } from '@lib/logging/install';
import { liveTail } from '@lib/logging/liveTail';
import {
  ConsoleEchoTransport,
  PipelineTransport,
  appendToBetterStack,
  appendToLiveTail,
  toStoreLevel,
  type Stage,
} from '@lib/logging/pipeline';
import Constants from 'expo-constants';
import * as Updates from 'expo-updates';
import { appLogFields } from '../app-shell/runId';
import { LogLayer, type LogLayerTransportParams } from 'loglayer';

type AppLogEntry = {
  level: string;
  text: string;
  source?: string;
  context?: Record<string, unknown>;
  ts: number;
};

// Snapshot console before installConsoleCapture patches it, so the echo
// transport writes to the real console rather than recursing into the capture.
const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
};

function stringify(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value instanceof Error)
    return value.stack ?? `${value.name}: ${value.message}`;
  try {
    return JSON.stringify(value);
  } catch {
    // Swallowed on purpose: this IS the logger, so there is no sink to report
    // a serialization failure to, and throwing here would lose the log line
    // AND the caller's control flow.
    return String(value);
  }
}

function serialize(p: LogLayerTransportParams): AppLogEntry {
  const record: Record<string, unknown> =
    p.hasData && p.data && typeof p.data === 'object' ? p.data : {};
  const { source, ts, ...context } = record;
  return {
    level: toStoreLevel(p.logLevel),
    text: p.messages.map(stringify).join(' '),
    source: typeof source === 'string' && source !== '' ? source : undefined,
    context,
    ts: typeof ts === 'number' ? ts : Date.now(),
  };
}

// Static references, so Expo inlines them into the bundle; a dynamic read finds nothing on a device.
const betterStackToken = process.env.EXPO_PUBLIC_BETTERSTACK_TOKEN;
const betterStackHost = process.env.EXPO_PUBLIC_BETTERSTACK_HOST;

const stages: Stage<AppLogEntry>[] = [appendToLiveTail() as Stage<AppLogEntry>];
if (betterStackToken && betterStackHost) {
  stages.push(
    appendToBetterStack({
      host: betterStackHost,
      token: betterStackToken,
      // debug lines every 2 s from every device would spend the ingest quota, and a 429 drops a batch
      minLevel: 'info',
      fields: Object.assign(appLogFields, {
        app: 'mla-tamagui',
        platform: process.env.EXPO_OS,
        version: Constants.expoConfig?.version,
        updateId: Updates.updateId ?? undefined,
        runtimeVersion: Updates.runtimeVersion ?? undefined,
        // the other fields are equal on the phone and every CI emulator; this one tells their launches apart
        sessionId: Constants.sessionId,
      }),
    })
  );
}

const rootLogLayer = new LogLayer({
  transport: [
    new PipelineTransport<AppLogEntry>({ serialize, stages }),
    new ConsoleEchoTransport({ console: originalConsole }),
  ],
});
rootLogLayer.setLevel('debug');

configureLogger({ root: () => rootLogLayer });

// Route console.* into the same tail, so a stray console call from a
// dependency is visible rather than invisible.
installConsoleCapture({ target: liveTail });

// One line at boot proves the chain end to end: if the debug sheet is empty,
// the wiring is broken rather than the app being quiet.
Logger.info('app.boot', { source: 'AppShell' });

// Dev only: the debug sheet's tail for a page script. A driven run reads the same lines off the console
// (ConsoleEchoTransport): `wf browser get-logs --level info`.
if (__DEV__ && typeof globalThis !== 'undefined') {
  (globalThis as { __liveTail?: typeof liveTail }).__liveTail = liveTail;
}
