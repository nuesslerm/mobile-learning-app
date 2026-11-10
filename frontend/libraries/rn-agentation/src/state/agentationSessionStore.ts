// agentationSessionStore.ts
import { create } from 'zustand';
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware';

declare const require: (id: string) => any;

function resolveStorage(): StateStorage {
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const mod = require('@react-native-async-storage/async-storage');
    return (mod.default ?? mod) as StateStorage;
  } catch {
    const mem = new Map<string, string>();
    return {
      getItem: (k) => Promise.resolve(mem.get(k) ?? null),
      setItem: (k, v) => {
        mem.set(k, v);
        return Promise.resolve();
      },
      removeItem: (k) => {
        mem.delete(k);
        return Promise.resolve();
      },
    };
  }
}

interface SessionState {
  sessionId: string | null;
  hydrated: boolean;
  setSessionId: (id: string) => void;
  clearSession: () => void;
  setHydrated: () => void;
}

export const useAgentationSessionStore = create<SessionState>()(
  persist(
    (set) => ({
      sessionId: null,
      hydrated: false,
      setSessionId: (sessionId) => set({ sessionId }),
      clearSession: () => set({ sessionId: null }),
      setHydrated: () => set({ hydrated: true }),
    }),
    {
      name: 'rn-agentation-session',
      version: 1,
      storage: createJSONStorage(resolveStorage),
      partialize: (s) => ({ sessionId: s.sessionId }),
      onRehydrateStorage: () => (state) => state?.setHydrated(),
    },
  ),
);
