// @vitest-environment jsdom
import { act, createRef, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import type { WebView } from 'react-native-webview';
import { expect, it, vi } from 'vitest';

import { EditorPage, PreparedEditorPages } from '../editor/EditorPage';

vi.mock('react-native', () => ({
  Platform: { OS: 'ios' },
  UIManager: { hasViewManagerConfig: () => true },
  StyleSheet: { absoluteFill: {} },
  View: ({ children }: { children: ReactNode }) => children,
}));
vi.mock('react-native-teleport', () => ({
  PortalProvider: ({ children }: { children: ReactNode }) => children,
  Portal: ({ children }: { children: ReactNode }) => children,
  PortalHost: () => null,
}));
vi.mock('react-native-webview', async () => {
  const { useImperativeHandle } = await import('react');
  return { WebView: ({ ref, onMessage }: { ref: React.Ref<unknown>; onMessage: (e: { nativeEvent: { data: string } }) => void }) => {
    useImperativeHandle(ref, () => ({ reload() {} }), []);
    return <button onClick={() => onMessage({ nativeEvent: { data: '{"type":"ready"}' } })} />;
  } };
});

it('Given a loaded page, when two editors open in succession, then each receives its own page and readiness message', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const container = document.createElement('div');
  const root = createRoot(container);
  const source = { uri: 'file:///editor.html' };
  const first = createRef<WebView>();
  const second = createRef<WebView>();
  const firstMessages = vi.fn();
  const secondMessages = vi.fn();
  const editor = (key: string, nativeRef: typeof first, onData: typeof firstMessages) =>
    <EditorPage key={key} canvas source={source} nativeRef={nativeRef} backgroundColor="black" onData={onData} onLoadStart={() => {}} onTerminate={() => {}} />;
  const render = (children: ReactNode) => act(async () => root.render(<PreparedEditorPages getSource={() => source}>{children}</PreparedEditorPages>));
  try {
    await render(null);
    const parked = container.querySelector('button')!;
    await act(async () => parked.click());
    await render(editor('first', first, firstMessages));
    expect(first.current).toBeTruthy();
    expect(firstMessages).toHaveBeenCalledExactlyOnceWith('{"type":"ready"}');
    const firstPage = first.current;
    const spare = container.querySelectorAll('button')[1]!;
    await render(editor('second', second, secondMessages));
    expect(first.current).toBeNull();
    expect(second.current).toBeTruthy();
    expect(second.current).not.toBe(firstPage);
    expect(secondMessages).not.toHaveBeenCalled();
    await act(async () => spare.click());
    expect(secondMessages).toHaveBeenCalledExactlyOnceWith('{"type":"ready"}');
    expect(firstMessages).toHaveBeenCalledTimes(1);
    expect(container.querySelectorAll('button')).toHaveLength(2);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
