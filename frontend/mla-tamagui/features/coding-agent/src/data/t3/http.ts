import type { Pairing } from '../../domain/mobile';
import { RcaUnavailable, WorktreeMissing } from '../../domain/t3Mobile';

export class HttpError extends Error {
  constructor(readonly status: number) {
    super('The server declined the request (HTTP ' + status + ').');
  }
}

export async function json(base: string, path: string, init?: RequestInit) {
  const controller = new AbortController();
  const cancel = () => controller.abort();
  const signal = init?.signal;
  if (signal?.aborted) cancel();
  signal?.addEventListener('abort', cancel);
  const timeout = setTimeout(cancel, 15000);
  let response: Response;
  try {
    response = await fetch(base + path, { ...init, signal: controller.signal });
  } catch (cause) {
    if (signal?.aborted) throw cause;
    throw new RcaUnavailable();
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener('abort', cancel);
  }
  if ([502, 503, 504].includes(response.status)) throw new RcaUnavailable();
  if (!response.ok) {
    const body = await response.json().catch((cause: unknown) => {
      if (cause instanceof SyntaxError) return null;
      throw cause;
    });
    if (response.status === 404 && body?.code === 'CHECKOUT_NOT_FOUND')
      throw new WorktreeMissing();
    if (
      body?._tag === 'UnsupportedOperationError' &&
      body?.method === 'preview'
    )
      throw new Error(
        'This host could not start the browser. Check that Chromium and browser streaming are available on the desktop, then try again.'
      );
    throw new HttpError(response.status);
  }
  return response.json() as Promise<unknown>;
}

export const authorization = (pair: Pairing) => ({
  Authorization: 'Bearer ' + pair.token,
});
