import type { LoginCaptureStatus } from '../contracts/ports';

export function deriveCaptureBannerMessage(
  status: LoginCaptureStatus,
  now: number
): string | null {
  if (!status.active || status.expiresAt === null) return null;
  const minutesLeft = Math.max(
    0,
    Math.floor((status.expiresAt - now) / 60_000)
  );
  return `Login-capture in progress — expires in ${minutesLeft}m.`;
}
