import { appConfig } from '@lib/app-config';
import type {
  InterpretResponse,
  PollResult,
  RunResponse,
  SubmitResponse,
  TestCase,
  TestResult,
} from '../domain';
import { Logger } from '@lib/logging';
import {
  allowedTraceparentOrigins,
  withTraceparent,
} from '@lib/logging/primitives';

import type {
  LeetCodeGatewayPort,
  LoginCaptureStatus,
  UserStatusResult,
} from '../contracts/ports';

export type { LoginCaptureStatus, UserStatusResult };

// Checked against the app's own configured leetcode-backend origin (not
// trusted blindly from the `backendBaseUrl` param callers pass in) so a bug
// elsewhere that hands this file a third-party URL still can't leak a
// traceparent header to it.
const allowedOrigins = allowedTraceparentOrigins([
  appConfig.leetcodeBackendBaseUrl,
]);

/**
 * Map the backend's `/run` payload (`{ok, results, error}`) onto the app's
 * `RunResponse`. Pure — the server did the execution, this just derives the
 * summary fields the results UI reads (status_msg, totals).
 */
function toRunResponse(raw: {
  ok?: boolean;
  results?: TestResult[];
  error?: string;
}): RunResponse {
  const results = raw.results ?? [];
  const totalCorrect = results.filter((r) => r.pass).length;
  const totalTestcases = results.length;
  if (!raw.ok || raw.error) {
    const errorMsg = raw.error ?? 'Unknown error';
    const isTimeout = errorMsg.includes('Timeout');
    return {
      source: 'backend-run',
      kind: 'test',
      state: 'FAILURE',
      requestId: '',
      run_success: false,
      status_msg: 'Error',
      total_correct: totalCorrect,
      total_testcases: totalTestcases,
      compile_error: isTimeout ? undefined : errorMsg,
      runtime_error: isTimeout ? errorMsg : undefined,
      results,
    };
  }
  const accepted = totalCorrect === totalTestcases;
  return {
    source: 'backend-run',
    kind: 'test',
    state: accepted ? 'SUCCESS' : 'FAILURE',
    requestId: '',
    run_success: true,
    status_msg: accepted ? 'Accepted' : 'Wrong Answer',
    total_correct: totalCorrect,
    total_testcases: totalTestcases,
    results,
  };
}

/**
 * Thin, pure fetch wrapper both platforms use to call backend/leetcode-backend.
 * Kept separate from the React component so the request/response handling is
 * unit-testable without rendering anything. The backend owns
 * the LeetCode session server-side, so
 * no function here takes a cookie — every call is just `{backendBaseUrl,
 * ...params}` and a 401 means "no valid session", surfaced as a normal
 * rejected promise.
 */

async function parseJsonOrThrow<T>(response: {
  ok: boolean;
  status?: number;
  json: () => Promise<unknown>;
}): Promise<T> {
  const payload = (await response.json()) as {
    error?: unknown;
  };
  if (!response.ok) {
    throw new Error(
      typeof payload?.error === 'string'
        ? payload.error
        : `Request failed (${response.status})`
    );
  }
  return payload as T;
}

// No active mobile span exists yet (pre-U12), so `withTraceparent` always
// sends no header today — wired now so it appears automatically once U12
// wires a real OTel span into this call path.
// requireBearerToken.ts on the backend rejects every /api/leetcode/* request
// without this -- unset in local dev unless the local backend is also
// configured with a matching VM_GATEWAY_AUTH_SEED to exercise the check.
function authHeader(): Record<string, string> {
  return appConfig.leetcodeBackendToken
    ? { Authorization: `Bearer ${appConfig.leetcodeBackendToken}` }
    : {};
}

type Endpoint =
  | 'user-status'
  | 'login-session'
  | 'test'
  | 'submit'
  | 'poll'
  | 'run'
  | 'format';

// The status is read before the body is parsed, so a proxy's HTML 502 still
// logs its status; a rejected fetch logs status 0.
async function sendJson<T>(
  endpoint: Endpoint,
  method: 'GET' | 'POST',
  send: () => Promise<Response>
): Promise<T> {
  const startedAt = Date.now();
  let status = 0;
  try {
    const response = await send();
    status = response.status;
    return await parseJsonOrThrow<T>(response);
  } catch (err) {
    Logger.withError(err).warn('leetcode.api.fail', {
      endpoint,
      method,
      status,
      ms: Date.now() - startedAt,
    });
    throw err;
  }
}

async function getJson<T>(endpoint: Endpoint, url: string): Promise<T> {
  return sendJson<T>(endpoint, 'GET', () =>
    fetch(
      url,
      withTraceparent(
        url,
        { method: 'GET', headers: authHeader() },
        null,
        allowedOrigins
      )
    )
  );
}

async function postJson<T>(
  endpoint: Endpoint,
  url: string,
  body: unknown
): Promise<T> {
  return sendJson<T>(endpoint, 'POST', () =>
    fetch(
      url,
      withTraceparent(
        url,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', ...authHeader() },
          body: JSON.stringify(body),
        },
        null,
        allowedOrigins
      )
    )
  );
}

export async function fetchUserStatus(
  backendBaseUrl: string
): Promise<UserStatusResult> {
  const payload = await getJson<{
    isSignedIn?: boolean;
    username?: string | null;
  }>('user-status', `${backendBaseUrl}/api/leetcode/user-status`);
  return {
    isSignedIn: Boolean(payload.isSignedIn),
    username: payload.username ?? null,
  };
}

export async function fetchTest(
  backendBaseUrl: string,
  params: {
    slug: string;
    lang: string;
    questionId: number;
    code: string;
    dataInput: string;
  }
): Promise<InterpretResponse> {
  return postJson<InterpretResponse>(
    'test',
    `${backendBaseUrl}/api/leetcode/test`,
    params
  );
}

export async function fetchSubmit(
  backendBaseUrl: string,
  params: { slug: string; lang: string; questionId: number; code: string }
): Promise<SubmitResponse> {
  return postJson<SubmitResponse>(
    'submit',
    `${backendBaseUrl}/api/leetcode/submit`,
    params
  );
}

export async function fetchPoll(
  backendBaseUrl: string,
  id: string
): Promise<PollResult> {
  return getJson<PollResult>(
    'poll',
    `${backendBaseUrl}/api/leetcode/poll/${id}`
  );
}

export async function fetchRunLocal(
  backendBaseUrl: string,
  params: {
    lang: string;
    code: string;
    methodName: string;
    testCases: TestCase[];
  }
): Promise<RunResponse> {
  const raw = await postJson<{
    ok?: boolean;
    results?: TestResult[];
    error?: string;
  }>('run', `${backendBaseUrl}/api/leetcode/run`, params);
  return toRunResponse(raw);
}

export async function fetchFormat(
  backendBaseUrl: string,
  params: { lang: string; code: string }
): Promise<string> {
  const raw = await postJson<{ formatted?: string }>(
    'format',
    `${backendBaseUrl}/api/leetcode/format`,
    params
  );
  return raw.formatted ?? '';
}

export async function fetchLoginCaptureStatus(
  backendBaseUrl: string
): Promise<LoginCaptureStatus> {
  return getJson<LoginCaptureStatus>(
    'login-session',
    `${backendBaseUrl}/api/leetcode/login-session`
  );
}

// The port declares one result shape (RunResponse) for every judge call, but
// test/submit/poll carry the wire id/state the fetch wrappers return; cast to
// the port-typed surface, which is what consumers and harness doubles agree on.
export function createLiveLeetCodeGateway(
  backendBaseUrl: string
): LeetCodeGatewayPort {
  return {
    userStatus: () => fetchUserStatus(backendBaseUrl),
    test: (p) => fetchTest(backendBaseUrl, p) as unknown as Promise<RunResponse>,
    submit: (p) =>
      fetchSubmit(backendBaseUrl, p) as unknown as Promise<RunResponse>,
    poll: (id) =>
      fetchPoll(backendBaseUrl, id) as unknown as Promise<RunResponse>,
    runLocal: (p) => fetchRunLocal(backendBaseUrl, p),
    format: (p) => fetchFormat(backendBaseUrl, p),
    loginCaptureStatus: () => fetchLoginCaptureStatus(backendBaseUrl),
  };
}
