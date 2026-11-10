/**
 * A fake `opencode serve`: the HTTP and server-sent-event surface the service's opencode gateway speaks (the SDK's session,
 * event, question and permission calls plus the raw `/mcp`, `/command`, `/api/model` and `/find/file` fetches). It records
 * every request and lets a test script what the "model" does when a prompt arrives, so the service's chat, control and
 * question routes are pinned from the outside against the protocol rather than against the service's own mock.
 */
import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from 'node:http';

export type RecordedRequest = {
  method: string;
  path: string;
  query: Record<string, string>;
  headers: Record<string, string | string[] | undefined>;
  body: unknown;
};

export type OpencodeEvent = {
  type: string;
  properties: Record<string, unknown>;
};

export type PromptScript = (ctx: {
  sessionID: string;
  body: Record<string, unknown>;
  emit: (event: OpencodeEvent) => void;
}) => void | Promise<void>;

export type FakeOpencode = {
  url: string;
  requests: RecordedRequest[];
  /** Requests whose path matches, in arrival order. */
  find: (method: string, path: RegExp | string) => RecordedRequest[];
  emit: (event: OpencodeEvent) => void;
  /** Replaces what happens when `prompt_async` arrives. */
  onPrompt: (script: PromptScript) => void;
  /** Overrides the answer of a route: status and JSON body (null body = empty). */
  respond: (
    method: string,
    path: RegExp | string,
    status: number,
    body?: unknown
  ) => void;
  subscribers: () => number;
  /** Forgets requests, overrides and the session counter, and restores the default (idle at once) prompt script. */
  reset: () => void;
  stop: () => Promise<void>;
};

const json = (res: ServerResponse, status: number, body: unknown): void => {
  const text = body === undefined ? '' : JSON.stringify(body);
  res.writeHead(status, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(text),
  });
  res.end(text);
};

const readBody = (req: IncomingMessage): Promise<string> =>
  new Promise((resolve) => {
    let data = '';
    req.on('data', (c) => (data += String(c)));
    req.on('end', () => resolve(data));
  });

export const sessionInfo = (id: string, directory = '/work') => ({
  id,
  slug: id,
  projectID: 'p1',
  directory,
  title: 'fake',
  version: '1',
  time: { created: 1, updated: 1 },
});

export async function startFakeOpencode(): Promise<FakeOpencode> {
  const requests: RecordedRequest[] = [];
  const clients = new Set<ServerResponse>();
  const overrides: {
    method: string;
    path: RegExp | string;
    status: number;
    body?: unknown;
  }[] = [];
  const defaultScript: PromptScript = ({ sessionID, emit }) => {
    emit({ type: 'session.idle', properties: { sessionID } });
  };
  let script: PromptScript = defaultScript;
  let sessions = 0;
  const connectsAtCreate = new Map<string, number>();

  // A prompt script starts once a subscription that opened after its session was created exists (plus a beat for any sibling
  // subscription), as a real server's events, which take seconds, do: a service that subscribes before it prompts and one that
  // subscribes after both get the script. For a session this server did not create, the anchor is the prompt. Events go to
  // every connected client, like a real server's.
  const pending: OpencodeEvent[] = [];
  let connects = 0;
  const emit = (event: OpencodeEvent): void => {
    if (clients.size === 0) pending.push(event);
    else
      for (const client of clients)
        client.write(`data: ${JSON.stringify(event)}\n\n`);
  };
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  const handle = async (
    req: IncomingMessage,
    res: ServerResponse
  ): Promise<void> => {
    const url = new URL(req.url ?? '/', 'http://x');
    const raw = await readBody(req);
    let body: unknown = undefined;
    if (raw.length > 0) {
      try {
        body = JSON.parse(raw);
      } catch {
        body = raw;
      }
    }
    const method = req.method ?? 'GET';
    requests.push({
      method,
      path: url.pathname,
      query: Object.fromEntries(url.searchParams),
      headers: req.headers,
      body,
    });

    const match = overrides.find(
      (o) =>
        o.method === method &&
        (typeof o.path === 'string'
          ? o.path === url.pathname
          : o.path.test(url.pathname))
    );
    if (match) return json(res, match.status, match.body);

    if (method === 'GET' && url.pathname === '/event') {
      res.writeHead(200, {
        'content-type': 'text/event-stream',
        'cache-control': 'no-cache',
        connection: 'keep-alive',
      });
      res.write(
        `data: ${JSON.stringify({ type: 'server.connected', properties: {} })}\n\n`
      );
      clients.add(res);
      connects += 1;
      res.on('close', () => clients.delete(res));
      for (const event of pending.splice(0)) emit(event);
      return;
    }
    if (method === 'GET' && url.pathname === '/find/file')
      return json(res, 200, []);
    if (method === 'POST' && url.pathname === '/session') {
      sessions += 1;
      connectsAtCreate.set(`oc-ses-${sessions}`, connects);
      return json(
        res,
        200,
        sessionInfo(
          `oc-ses-${sessions}`,
          url.searchParams.get('directory') ?? '/work'
        )
      );
    }
    const sessionRoute = url.pathname.match(/^\/session\/([^/]+)(?:\/(.+))?$/);
    if (sessionRoute) {
      const id = decodeURIComponent(sessionRoute[1] ?? '');
      const action = sessionRoute[2];
      if (!action && (method === 'GET' || method === 'PATCH'))
        return json(res, 200, sessionInfo(id));
      if (action === 'abort' && method === 'POST') return json(res, 200, true);
      if (action === 'prompt_async' && method === 'POST') {
        res.writeHead(204);
        res.end();
        const seen = connectsAtCreate.get(id) ?? connects;
        const prompt = (body ?? {}) as Record<string, unknown>;
        void (async () => {
          const deadline = Date.now() + 5_000;
          while (connects === seen && Date.now() < deadline) await sleep(5);
          await sleep(100);
          await script({ sessionID: id, body: prompt, emit });
        })();
        return;
      }
    }
    if (
      method === 'POST' &&
      /^\/api\/session\/[^/]+\/prompt$/.test(url.pathname)
    )
      return json(res, 200, {});
    if (
      method === 'POST' &&
      /^\/api\/session\/[^/]+\/prompt$/.test(url.pathname)
    )
      return json(res, 200, {});
    if (
      method === 'POST' &&
      /^\/question\/[^/]+\/(reply|reject)$/.test(url.pathname)
    )
      return json(res, 200, true);
    if (method === 'POST' && /^\/permission\/[^/]+\/reply$/.test(url.pathname))
      return json(res, 200, true);
    if (method === 'GET' && url.pathname === '/mcp') return json(res, 200, {});
    if (method === 'GET' && url.pathname === '/command')
      return json(res, 200, []);
    if (method === 'GET' && url.pathname === '/api/model')
      return json(res, 200, []);
    return json(res, 404, {
      error: `fake opencode: no route for ${method} ${url.pathname}`,
    });
  };

  const server: Server = createServer((req, res) => {
    handle(req, res).catch((err: unknown) => {
      res.statusCode = 500;
      res.end(String(err));
    });
  });

  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;

  return {
    url: `http://127.0.0.1:${port}`,
    requests,
    find: (method, path) =>
      requests.filter(
        (r) =>
          r.method === method &&
          (typeof path === 'string' ? r.path === path : path.test(r.path))
      ),
    emit,
    onPrompt: (next) => {
      script = next;
    },
    respond: (method, path, status, body) => {
      overrides.unshift({ method, path, status, body });
    },
    subscribers: () => clients.size,
    reset: () => {
      requests.length = 0;
      overrides.length = 0;
      sessions = 0;
      connectsAtCreate.clear();
      script = defaultScript;
    },
    stop: () =>
      new Promise<void>((resolve) => {
        for (const client of clients) client.end();
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}
