import { describe, it, expect, beforeEach, vi } from 'vitest';

import { useLeetCodeAuthStore } from '../authStore';

// zustand's `persist` middleware fire-and-forgets a write to AsyncStorage on
// every setState call. The real package's web shim reaches for `window`,
// which isn't defined in this suite's `node` vitest environment — stub it
// with an in-memory map so store logic can be tested without a storage
// backend.
vi.mock('@react-native-async-storage/async-storage', () => {
  const store = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => store.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: async (key: string) => {
        store.delete(key);
      },
    },
  };
});

describe('useLeetCodeAuthStore', () => {
  beforeEach(() => {
    useLeetCodeAuthStore.setState({ isLoggedIn: false, username: null });
  });

  it('Given default state, When setStatus(true, username) called, Then store marks logged in with that username', () => {
    useLeetCodeAuthStore.getState().setStatus(true, 'alice');

    const state = useLeetCodeAuthStore.getState();
    expect(state.isLoggedIn).toBe(true);
    expect(state.username).toBe('alice');
  });

  it('Given default state, When setStatus(false, ...) called, Then store clears username even if one is passed', () => {
    useLeetCodeAuthStore.getState().setStatus(false, 'alice');

    const state = useLeetCodeAuthStore.getState();
    expect(state.isLoggedIn).toBe(false);
    expect(state.username).toBeNull();
  });

  it('Given logged-in state, When clearCredentials called, Then store resets the local display cache', () => {
    useLeetCodeAuthStore.getState().setStatus(true, 'alice');
    useLeetCodeAuthStore.getState().clearCredentials();

    const state = useLeetCodeAuthStore.getState();
    expect(state.isLoggedIn).toBe(false);
    expect(state.username).toBeNull();
  });
});
