export type LoginStatus = {
  isSignedIn: boolean;
  username: string | null;
};

export function deriveLoginStatusMessage(status: LoginStatus): string {
  if (status.isSignedIn && status.username) {
    return `Signed in as ${status.username}`;
  }
  return 'Not connected — run a login-capture session.';
}
