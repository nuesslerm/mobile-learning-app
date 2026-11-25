// Import first in app/_layout.tsx so the host is configured before any Logger call.
import { Logger, configureLogger } from '@lib/logging';
import {
  ConsoleEchoTransport,
  PipelineTransport,
  appendToBetterStack,
  toStoreLevel,
} from '@lib/logging/pipeline';
import Constants from 'expo-constants';
import * as Updates from 'expo-updates';
import { appLogFields } from '../app-shell/runId';
import { LogLayer, type LogLayerTransport, type LogLayerTransportParams } from 'loglayer';
import { Platform } from 'react-native';

type AppLogEntry = {
  level: string;
  text: string;
  source?: string;
  context?: Record<string, unknown>;
  ts: number;
};

if (__DEV__ && Platform.OS !== 'web') {
  (require('@buoy-gg/console') as typeof import('@buoy-gg/console')).installConsoleCapture();
}

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

const manifest = Updates.manifest;
const metadata = manifest && 'metadata' in manifest ? manifest.metadata : undefined;
const updateGroup = metadata && 'updateGroup' in metadata && typeof metadata.updateGroup === 'string'
  ? metadata.updateGroup
  : undefined;

const transports: LogLayerTransport[] = [new ConsoleEchoTransport({ console })];
if (betterStackToken && betterStackHost) {
  const forward = appendToBetterStack({
    host: betterStackHost,
    token: betterStackToken,
    // debug lines every 2 s from every device would spend the ingest quota, and a 429 drops a batch
    minLevel: 'info',
    fields: Object.assign(appLogFields, {
      app: 'mla-tamagui',
      platform: process.env.EXPO_OS,
      version: Constants.expoConfig?.version,
      updateId: Updates.updateId ?? undefined,
      updateGroup,
      embeddedUpdate: Updates.isEmbeddedLaunch,
      nativeBuildVersion: Constants.platform?.ios?.buildNumber ?? Constants.platform?.android?.versionCode,
      runtimeVersion: Updates.runtimeVersion ?? undefined,
      // the other fields are equal on the phone and every CI emulator; this one tells their launches apart
      sessionId: Constants.sessionId,
    }),
  });
  transports.push(new PipelineTransport<AppLogEntry>({ serialize, stages: [forward] }));
}

const rootLogLayer = new LogLayer({ transport: transports });
rootLogLayer.setLevel('debug');
configureLogger({ root: () => rootLogLayer });

Logger.info('app.boot', { source: 'AppShell' });
