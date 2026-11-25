import { z } from 'zod';
const urlString = z.url();

export function parseT3PairingInvite(
  input: string
): { endpoint: string; code: string } | null {
  const parsed = urlString.safeParse(input.trim());
  if (!parsed.success) return null;
  const url = new URL(parsed.data);
  if (
    !['https:', 'http:'].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.search ||
    !url.pathname.endsWith('/pair')
  )
    return null;
  const tokens = new URLSearchParams(url.hash.slice(1)).getAll('token');
  if (tokens.length !== 1 || !tokens[0].trim()) return null;
  return { endpoint: url.origin + url.pathname.slice(0, -5), code: tokens[0] };
}
