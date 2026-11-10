import { describe, it, expect } from 'vitest';

import { deriveLoginStatusMessage } from '../loginStatusMessage';

describe('deriveLoginStatusMessage', () => {
  it('given signed in user with username when called then shows username', () => {
    expect(
      deriveLoginStatusMessage({ isSignedIn: true, username: 'alice' })
    ).toBe('Signed in as alice');
  });

  it('given signed out user when called then shows not connected message', () => {
    expect(
      deriveLoginStatusMessage({ isSignedIn: false, username: null })
    ).toBe('Not connected — run a login-capture session.');
  });

  it('given signed in but missing username when called then shows not connected message', () => {
    expect(
      deriveLoginStatusMessage({ isSignedIn: true, username: null })
    ).toBe('Not connected — run a login-capture session.');
  });
});
