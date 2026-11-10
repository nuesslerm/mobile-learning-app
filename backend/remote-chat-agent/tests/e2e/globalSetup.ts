/**
 * Refuses to run the e2e suite without a real opencode at the far end.
 *
 * WHY. An end-to-end test whose far end is a mock is an integration test wearing
 * the name of one that would have caught the outage: the bug that actually broke
 * the agent in production (opencode resolving a working directory that exists
 * only in the caller's container, yielding a stream with zero content) is
 * impossible to reproduce against a mock that never returns zero content. The
 * service has no mock gateway any more, so the rule is simply that something
 * real must be listening.
 *
 * Against an existing instance (`REMOTE_CHAT_AGENT_BASE_URL`, the deployed
 * service) the instance reaches its own opencode, so only its health is checked
 * when the suite starts it.
 */
const REAL_OPENCODE_URL =
  process.env['OPENCODE_SERVER_URL'] ?? 'http://localhost:4096';

export default async function globalSetup(): Promise<void> {
  if (process.env['REMOTE_CHAT_AGENT_BASE_URL']?.trim()) return;

  const reachable = await isReachable(REAL_OPENCODE_URL);
  if (reachable) return;

  throw new Error(
    [
      '',
      `No opencode server is reachable at ${REAL_OPENCODE_URL}, so this suite`,
      'cannot run end to end.',
      '',
      'Start a real opencode server, then re-run:',
      '  opencode serve --port 4096',
      '',
      'Or point the suite at one:',
      '  OPENCODE_SERVER_URL=http://<host>:<port> pnpm --filter @app/remote-chat-agent run test:e2e',
      '',
      'Or run it against the deployed service, which brings its own opencode:',
      '  REMOTE_CHAT_AGENT_BASE_URL=http://remote-chat-agent-prod:8787 pnpm --filter @app/remote-chat-agent run test:e2e',
      '',
    ].join('\n')
  );
}

async function isReachable(baseUrl: string): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 3000);
  try {
    // Any HTTP answer proves something is listening and speaking HTTP; the
    // suite's own assertions cover whether it behaves like opencode.
    await fetch(`${baseUrl}/app`, { signal: controller.signal });
    return true;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}
