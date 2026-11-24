import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';

describe('webBridgeClient', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.resetModules();
    vi.doUnmock('@lib/app-config');
  });

  beforeEach(() => {
    vi.resetModules();
    vi.doMock('@lib/app-config', () => ({
      appConfig: {},
    }));
  });

  it('Given mock fetch returning user-status, When fetchUserStatus called, Then returns isSignedIn and username from backend', async () => {
    const { fetchUserStatus: fetchUserStatusLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ isSignedIn: true, username: 'alice' }),
    });

    const result = await fetchUserStatusLocal('http://localhost:8789');
    expect(result).toEqual({ isSignedIn: true, username: 'alice' });

    const [url, options] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe('http://localhost:8789/api/leetcode/user-status');
    expect(options).toEqual({ method: 'GET', headers: {} });
  });

  it('Given empty response from backend, When fetchUserStatus called, Then defaults to signed-out', async () => {
    const { fetchUserStatus: fetchUserStatusLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({}),
    });

    const result = await fetchUserStatusLocal('http://localhost:8789');
    expect(result).toEqual({ isSignedIn: false, username: null });
  });

  it('Given non-2xx response with error message, When fetchUserStatus called, Then throws with the backend error message', async () => {
    const { fetchUserStatus: fetchUserStatusLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 401,
      json: async () => ({ error: 'not signed in' }),
    });

    await expect(fetchUserStatusLocal('http://localhost:8789')).rejects.toThrow(
      'not signed in'
    );
  });

  it('Given test parameters, When fetchTest called, Then posts the payload with no cookie field', async () => {
    const { fetchTest: fetchTestLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ interpret_id: 'abc' }),
    });

    const result = await fetchTestLocal('http://localhost:8789', {
      slug: 'two-sum',
      lang: 'python3',
      questionId: 1,
      code: 'pass',
      dataInput: '[]',
    });
    expect(result).toEqual({ interpret_id: 'abc' });

    const [url, options] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe('http://localhost:8789/api/leetcode/test');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({
      slug: 'two-sum',
      lang: 'python3',
      questionId: 1,
      code: 'pass',
      dataInput: '[]',
    });
  });

  it('Given submit parameters, When fetchSubmit called, Then posts the payload with no cookie field', async () => {
    const { fetchSubmit: fetchSubmitLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ submission_id: '999' }),
    });

    const result = await fetchSubmitLocal('http://localhost:8789', {
      slug: 'two-sum',
      lang: 'python3',
      questionId: 1,
      code: 'pass',
    });
    expect(result).toEqual({ submission_id: '999' });

    const [url, options] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe('http://localhost:8789/api/leetcode/submit');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({
      slug: 'two-sum',
      lang: 'python3',
      questionId: 1,
      code: 'pass',
    });
  });

  it('Given poll id, When fetchPoll called, Then GETs the poll endpoint with id in path and no body', async () => {
    const { fetchPoll: fetchPollLocal } = await import(
      '../webBridgeClient'
    );
    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ state: 'SUCCESS' }),
    });

    const result = await fetchPollLocal('http://localhost:8789', '123456');
    expect(result).toEqual({ state: 'SUCCESS' });

    const [url, options] = (globalThis.fetch as ReturnType<typeof vi.fn>).mock
      .calls[0];
    expect(url).toBe('http://localhost:8789/api/leetcode/poll/123456');
    expect(options).toEqual({ method: 'GET', headers: {} });
  });
});

describe('auth header', () => {
  afterEach(() => {
    vi.resetModules();
    vi.doUnmock('@lib/app-config');
  });

  it('Given leetcodeBackendToken configured, When fetchUserStatus called, Then sends Authorization: Bearer header', async () => {
    vi.resetModules();
    vi.doMock('@lib/app-config', () => ({
      appConfig: { leetcodeBackendToken: 'test-token' },
    }));
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ isSignedIn: false, username: null }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const { fetchUserStatus: fetchUserStatusWithToken } =
      await import('../webBridgeClient');
    await fetchUserStatusWithToken('http://localhost:8789');

    const [, options] = fetchMock.mock.calls[0];
    expect(options.headers).toEqual({ Authorization: 'Bearer test-token' });
  });
});

describe('fetchLoginCaptureStatus', () => {
  afterEach(() => {
    vi.resetModules();
    vi.doUnmock('@lib/app-config');
  });

  it('Given /login-session endpoint, When fetchLoginCaptureStatus called, Then returns the capture status from GET', async () => {
    vi.resetModules();
    vi.doMock('@lib/app-config', () => ({
      appConfig: {},
    }));

    const { fetchLoginCaptureStatus: fetchLoginCaptureStatusLocal } =
      await import('../webBridgeClient');
    const fetchMock = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ active: true, expiresAt: 301_000 }),
    }));
    vi.stubGlobal('fetch', fetchMock);

    const result = await fetchLoginCaptureStatusLocal('http://backend');

    expect(fetchMock).toHaveBeenCalledWith(
      'http://backend/api/leetcode/login-session',
      { method: 'GET', headers: {} }
    );
    expect(result).toEqual({ active: true, expiresAt: 301_000 });
  });
});
