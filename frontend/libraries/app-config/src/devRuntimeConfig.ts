import { Logger } from '@lib/logging';
import type { Href } from 'expo-router';
import { z } from 'zod';
import rawConfig from '../dev-runtime-config.json';
import { LaunchArguments } from './nativeLaunchArgs';
export const ALLOWED_ROUTES = ['/', '/service-report', '/leetcode/login', '/leetcode/toolbar', '/leetcode/two-sum', '/leetcode/contains-duplicate', '/leetcode/group-anagrams'] as const satisfies readonly Href[];

const devRuntimeConfigSchema = z.object({
  initial_route: z.string().nullable().default(null).refine(v => v == null ||
  // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
  (ALLOWED_ROUTES as readonly string[]).includes(v)),
  feature_flags: z.record(z.string(), z.boolean()).default({})
});
export type DevRuntimeConfig = z.infer<typeof devRuntimeConfigSchema>;

/**
 * Read the `--devConfigJson` launch arg, which lets a developer (or a Maestro
 * canary) supply the entire dev runtime config inline at app launch — no need
 * to mutate the bundled `dev-runtime-config.json` on disk. Accepts either a
 * JSON string or a pre-parsed object (the native bridge auto-parses values
 * that look like JSON; see `expo-native-launch-arguments/build/index.js`).
 * Returns `null` when the arg is absent or unparseable.
 */
function readLaunchArgOverride(): unknown {
  try {
    const args = LaunchArguments.value<{
      devConfigJson?: unknown;
    }>();
    const raw = args.devConfigJson;
    if (raw == null) return null;
    if (typeof raw === 'string') {
      if (raw.length === 0) return null;
      return JSON.parse(raw);
    }
    // Already an object (bridge parsed it) — return as-is.
    if (typeof raw === 'object') return raw;
    return null;
  } catch (err) {
    Logger.withError(err).warn('dev_runtime_config.override_parse_failed');
    return null;
  }
}
const override = readLaunchArgOverride();
const candidate = override ?? rawConfig;

// A silently-degraded config (`initialRoute: null`) is a footgun: the app
// boots on the (app) Home tab and the developer sees a blank-looking
// screen with no obvious signal that their config was the cause. We
// expose the parse failure as `devRuntimeConfigError` so the layout can
// render a screaming full-screen crash page via `AppBootErrorBoundary`,
// instead of throwing here (which would crash the entire module init
// and bypass every error boundary, leaving the user with Expo Go's
// default blank screen).
const buildDevRuntimeConfig = (): DevRuntimeConfig => {
  const result = devRuntimeConfigSchema.safeParse(candidate);
  if (result.success) {
    devRuntimeConfigError = null;
    return result.data;
  }
  // Log the FULL Zod issue list (path + message + received value) so the
  // error is queryable in BetterStack without having to also load the
  // on-screen crash page. The previous shape (`err: ZodError`) only
  // serialized as `[ZodError: ...]` and lost the issues — which is what
  // masked the root cause for the entire debug session.
  Logger.withError(result.error).withMetadata({
    source: override != null ? 'launchArg' : 'file',
    issues: result.error.issues.map(i => ({
      path: i.path.join('.') || '(root)',
      code: i.code,
      message: i.message,
      // eslint-disable-next-line no-restricted-syntax -- local escape; switching to Zod/guards would not improve runtime safety here
      received: (i as {
        received?: unknown;
      }).received
    })),
    candidate
  }).error('dev_runtime_config.invalid');
  const err = new InvalidDevRuntimeConfigError(result.error, candidate);
  devRuntimeConfigError = err;
  return {
    initial_route: null,
    feature_flags: {}
  };
};
export class InvalidDevRuntimeConfigError extends Error {
  readonly zodError: z.ZodError;
  readonly candidate: unknown;
  readonly source: 'launchArg' | 'file';
  readonly allowedRoutes: readonly string[];
  constructor(zodError: z.ZodError, candidate: unknown) {
    const issues = zodError.issues.map(i => `${i.path.join('.') || '(root)'} — ${i.message}`).join('; ');
    const allowedList = ALLOWED_ROUTES.map(r => `  - ${r}`).join('\n');
    super(`dev-runtime-config invalid: ${issues}\n\nAllowed initial_route values:\n${allowedList}\n\n(Or null to boot on the default (app) Home.)`);
    this.name = 'InvalidDevRuntimeConfigError';
    this.zodError = zodError;
    this.candidate = candidate;
    this.source = override != null ? 'launchArg' : 'file';
    this.allowedRoutes = ALLOWED_ROUTES;
  }
}

/**
 * Non-null when the dev runtime config (either the bundled
 * `dev-runtime-config.json` or the inline `--devConfigJson` launch arg)
 * failed Zod validation. Read by `app/_layout.tsx` to decide whether to
 * render the full tree or `AppBootErrorBoundary` with a screaming crash
 * page. **Mutated at module init** — there's exactly one config parse,
 * so reading after the first render is safe.
 */
export let devRuntimeConfigError: InvalidDevRuntimeConfigError | null = null;
export const devRuntimeConfig: DevRuntimeConfig = buildDevRuntimeConfig();
export function isFeatureEnabled(name: string): boolean {
  return devRuntimeConfig.feature_flags[name] === true;
}