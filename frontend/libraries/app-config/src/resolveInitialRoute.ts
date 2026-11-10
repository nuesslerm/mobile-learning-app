export type InitialRouteSources = {
  /**
   * Value from `dev-runtime-config.json`'s `initial_route` field.
   * `null` when the field is absent or unconfigured (not `undefined` —
   * other sources use `undefined` for "not set", so callers can
   * distinguish "config missing" from "value not set").
   */
  readonly file: string | null;
  readonly envVar: string | undefined;
  readonly perEnvDefault: string | undefined;
  readonly isProduction: boolean;
};

/**
 * Resolve the initial route from three sources, in priority order:
 *   [1] Dev config   — persists across reloads, dev + staging only
 *   [2] Env var      — baked into the bundle at publish time (`wf ship eas --initial-route`)
 *   [3] Per-env default — last-resort fallback from CONFIG[appEnv]
 *
 * Pure function. No I/O. No module state. Trivially testable.
 */
export function resolveInitialRoute(
  sources: InitialRouteSources
): string | undefined {
  const selected =
    (!sources.isProduction && nonEmpty(sources.file)) ||
    nonEmpty(sources.envVar) ||
    nonEmpty(sources.perEnvDefault);

  return selected;
}

const nonEmpty = (v: string | null | undefined): string | undefined => {
  if (v == null) return undefined;
  return v.length > 0 ? v : undefined;
};
