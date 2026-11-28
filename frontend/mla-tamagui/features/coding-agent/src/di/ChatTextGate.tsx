import { ChatTextContext, type ChatTextPreference } from '@lib/skin';
import { Logger } from '@lib/logging';
import { useEffect, useSyncExternalStore, type ReactNode } from 'react';
import { Dimensions, useWindowDimensions } from 'react-native';
import type { SettingsPreferencesPort } from '../domain/ports';
import { useCodingAgentPorts } from '../state/ports';

function createChatTextStore(port: SettingsPreferencesPort) {
  let snapshot: Partial<ChatTextPreference> | null = null;
  const listeners = new Set<() => void>();
  let hydration: Promise<void> | undefined;
  let pending = Promise.resolve();
  let revision = 0;
  function publish(next: typeof snapshot) {
    snapshot = next;
    listeners.forEach((listener) => listener());
  }
  function load() {
    return (hydration ??= port
      .readChatText()
      .then(publish)
      .catch((cause) => {
        Logger.withError(cause).warn(
          'coding-agent.settings.load_chat_text_failed'
        );
      }));
  }
  function save(
    change: Partial<ChatTextPreference> | null,
    defaultSize: number
  ) {
    void load().then(() => {
      const next = change && {
        size: snapshot?.size ?? defaultSize,
        font: snapshot?.font ?? 'system',
        lineSpacing: snapshot?.lineSpacing ?? 'default',
        ...change,
      };
      if (
        snapshot?.size === next?.size &&
        snapshot?.font === next?.font &&
        snapshot?.lineSpacing === next?.lineSpacing &&
        Boolean(snapshot) === Boolean(next)
      )
        return;
      const version = ++revision;
      publish(next);
      pending = pending
        .then(() => port.writeChatText(next))
        .catch(async (cause) => {
          Logger.withError(cause).warn(
            'coding-agent.settings.save_chat_text_failed'
          );
          if (revision === version) {
            try {
              const saved = await port.readChatText();
              if (revision === version) publish(saved);
            } catch (readError) {
              Logger.withError(readError).warn(
                'coding-agent.settings.load_chat_text_failed'
              );
            }
          }
        });
    });
  }
  return {
    load,
    save,
    snapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

// The shell's chat stack and the standalone settings stack share one device preference.
const stores = new WeakMap<
  SettingsPreferencesPort,
  ReturnType<typeof createChatTextStore>
>();
function storeFor(port: SettingsPreferencesPort) {
  const existing = stores.get(port);
  if (existing) return existing;
  const store = createChatTextStore(port);
  stores.set(port, store);
  return store;
}

export function ChatTextGate({ children }: { children: ReactNode }) {
  const { preferences } = useCodingAgentPorts();
  const store = storeFor(preferences);
  useEffect(() => {
    void store.load();
  }, [store]);
  const stored = useSyncExternalStore(
    store.subscribe,
    store.snapshot,
    () => null
  );
  useWindowDimensions();
  const screen = Dimensions.get('screen');
  const defaultSize = Math.min(screen.width, screen.height) >= 600 ? 16 : 14;
  return (
    <ChatTextContext.Provider
      value={{
        value: {
          size: stored?.size ?? defaultSize,
          font: stored?.font ?? 'system',
          lineSpacing: stored?.lineSpacing ?? 'default',
        },
        stored: stored !== null,
        defaultSize,
        update: (change) => store.save(change, defaultSize),
        reset: () => store.save(null, defaultSize),
      }}
    >
      {children}
    </ChatTextContext.Provider>
  );
}
