/**
 * Minimal HTTP client for the agentation-mcp server.
 *
 * Pure fetch wrapper; throws on non-2xx so callers can decide whether to
 * swallow or surface the error.
 */
export interface AgentationHttpConfig {
  endpoint: string;
  fetchImpl?: typeof fetch;
}

export interface CreateSessionBody {
  domain?: string;
  page?: string;
}

export interface CreateSessionResponse {
  id: string;
}

export interface AnnotationPayload {
  kind: 'annotate' | 'placement';
  componentName: string;
  sourceFile?: string;
  sourceLine?: number;
  note?: string;
  rect?: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  wireframe?: {
    componentType: string;
    xRatio: number;
    yRatio: number;
    widthRatio: number;
    heightRatio: number;
  };
  documentNodeId?: string;
  documentNodeProps?: Record<string, unknown>;
  documentNodeSubtree?: unknown;
}

async function httpFetch(
  cfg: AgentationHttpConfig,
  path: string,
  options?: RequestInit,
): Promise<unknown> {
  const url = `${cfg.endpoint.replace(/\/$/, '')}${path}`;
  const res = await (cfg.fetchImpl ?? fetch)(url, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`agentation HTTP ${res.status}: ${body}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as unknown) : null;
}

export async function createSession(
  cfg: AgentationHttpConfig,
  body: CreateSessionBody,
): Promise<CreateSessionResponse> {
  return (await httpFetch(cfg, '/sessions', {
    method: 'POST',
    body: JSON.stringify(body),
  })) as CreateSessionResponse;
}

export async function postAnnotation(
  cfg: AgentationHttpConfig,
  sessionId: string,
  annotation: AnnotationPayload,
): Promise<void> {
  await httpFetch(cfg, `/sessions/${encodeURIComponent(sessionId)}/annotations`, {
    method: 'POST',
    body: JSON.stringify(annotation),
  });
}
