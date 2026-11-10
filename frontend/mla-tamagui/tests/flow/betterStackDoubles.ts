type GlobalHandler = (error: unknown, isFatal?: boolean) => void;
type Post = { url: string; method?: string; headers: Record<string, string>; items: Record<string, unknown>[] };

// The platform's fetch, doubled: it records every request and answers the ingest host as BetterStack does,
// at once or, with `hold`, when the test releases it.
export function fetchDouble(host: string, opts: { hold?: boolean } = {}) {
  const requests: string[] = [];
  const posts: Post[] = [];
  const held: (() => void)[] = [];
  const passThrough = globalThis.fetch;
  const fetch = (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
    const url = String(input);
    requests.push(`${init?.method ?? 'GET'} ${url}`);
    if (!url.startsWith(`https://${host}`)) return passThrough(input, init);
    posts.push({
      url,
      method: init?.method,
      headers: init?.headers as Record<string, string>,
      items: JSON.parse(String(init?.body)) as Record<string, unknown>[],
    });
    if (!opts.hold) return Promise.resolve(new Response(null, { status: 202 }));
    return new Promise((resolve) => held.push(() => resolve(new Response(null, { status: 202 }))));
  };
  const releaseAll = () => held.splice(0).forEach((release) => release());
  return { requests, posts, fetch, releaseAll };
}

// React Native's global handler, as InitializeCore leaves it before the app loads
export function nativeGlobalHandler(earlier: GlobalHandler = () => undefined) {
  let handler: GlobalHandler = earlier;
  (globalThis as { ErrorUtils?: unknown }).ErrorUtils = {
    getGlobalHandler: () => handler,
    setGlobalHandler: (next: GlobalHandler) => {
      handler = next;
    },
  };
  return (error: unknown, isFatal?: boolean) => handler(error, isFatal);
}
