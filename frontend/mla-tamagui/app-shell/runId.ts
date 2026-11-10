// The BetterStack forwarder reads these fields on every line it sends, so a run id set after start-up still reaches the next line.
export const appLogFields: Record<string, unknown> = { runId: undefined };

// `wf android test` opens its links with `?wfRun=<id>`, so the lines of a harness run are the ones carrying its id.
export function captureRunId(url: string): void {
  const id = /[?&]wfRun=([A-Za-z0-9-]+)/.exec(url)?.[1];
  if (id) appLogFields.runId = id;
}
