import { afterEach, describe, expect, it, vi } from 'vitest';

import { fetchRunLocal } from '../webBridgeClient';

afterEach(() => vi.unstubAllGlobals());

describe('fetchRunLocal', () => {
  it('given params carrying what only the device runner needs, when it posts, then the body is exactly the four fields the backend reads', async () => {
    // Given a backend that records the request body
    let body: unknown;
    vi.stubGlobal('fetch', async (_url: string, init: { body: string }) => {
      body = JSON.parse(init.body);
      return { ok: true, status: 200, json: async () => ({ ok: true, results: [] }) };
    });

    // When a run is posted with the widened params
    await fetchRunLocal('http://backend', { lang: 'python3', code: 'c', methodName: 'm', testCases: [], paramTypes: { head: 'ListNode' }, returnType: null, inPlace: 'matrix' } as Parameters<typeof fetchRunLocal>[1]);

    // Then nothing else leaves the device
    expect(Object.keys(body as object).sort()).toEqual(['code', 'lang', 'methodName', 'testCases']);
  });

  it('given the backend answers an error, when it is shaped, then the verdict is the shared one', async () => {
    vi.stubGlobal('fetch', async () => ({ ok: true, status: 200, json: async () => ({ ok: false, error: 'CompileError: x' }) }));
    expect(await fetchRunLocal('http://backend', { lang: 'python3', code: 'c', methodName: 'm', testCases: [] })).toMatchObject({ source: 'backend-run', status_msg: 'Error', compile_error: 'CompileError: x' });
  });
});
