// features/coding-agent/src/domain/wsUrl.ts
//
// Pure derivation of the WebSocket URL the client opens against the
// orchestration server. The agent publishes its HTTP base URL (so the same
// value also works for `/health`, the WS handshake, etc.); `toWsUrl` swaps
// the scheme (`http` → `ws`, `https` → `wss`), pins the path to `/ws`, and
// attaches the bearer token as `?token=` when provided. Strips any trailing
// slashes so `https://host/` and `https://host` resolve to the same URL.

const URL_HOST = /^[a-z][a-z0-9+.-]*:\/\/(?:[^@/?#]*@)?([^/?#]+)/i;

/** The host of a URL for a log field: never the userinfo, path or query, where a token rides. */
export function wsUrlHost(url: string): string {
  return URL_HOST.exec(url)?.[1] ?? 'unparsed';
}

/** An error's name for a log field, where its message would quote the URL (token included) or the frame being parsed. */
export function errorNameOf(cause: unknown): string {
  return cause instanceof Error ? cause.name : typeof cause;
}

export function toWsUrl(agentBaseUrl: string, token?: string): string {
  const trimmed = agentBaseUrl.trim().replace(/\/+$/, '');
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch (cause) {
    throw new Error(`Invalid agent base URL: ${agentBaseUrl}`, { cause });
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error(
      `Unsupported protocol for agent base URL: ${url.protocol}`,
    );
  }
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.pathname = '/ws';
  url.search = '';
  if (token !== undefined && token.length > 0) {
    url.searchParams.set('token', token);
  }
  return url.toString();
}
