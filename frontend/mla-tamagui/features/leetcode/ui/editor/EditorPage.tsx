import { Logger } from '@lib/logging';
import { createContext, useCallback, useContext, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from 'react';
import { Platform, StyleSheet, UIManager, View } from 'react-native';
import { WebView } from 'react-native-webview';
import { Portal, PortalHost, PortalProvider } from 'react-native-teleport';

type Source = { html: string } | { uri: string };
type PageProps = {
  source: Source;
  nativeRef: RefObject<WebView | null>;
  backgroundColor: string;
  beforeLoad?: string;
  canvas: boolean;
  onData: (raw: string) => void;
  onLoadStart: () => void;
  onTerminate: () => void;
};
type Owner = { host: string; props: RefObject<PageProps> };
type Page = { id: number; native: WebView | null; ready?: string; owner?: Owner };

class EditorPages {
  private nextId = 0;
  private spare = this.newPage();
  private pages: Page[] = [this.spare];
  private listeners = new Set<() => void>();

  constructor(readonly source: Source) {}

  private newPage(): Page {
    return { id: this.nextId++, native: null };
  }

  snapshot = () => this.pages;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  private publish(pages: Page[]) {
    this.pages = pages;
    for (const notify of this.listeners) notify();
  }

  attach(owner: Owner) {
    const page = this.spare;
    this.spare = this.newPage();
    page.owner = owner;
    owner.props.current.nativeRef.current = page.native;
    this.publish([...this.pages, this.spare]);
    Logger.info('editor.page_acquired', { ready: !!page.ready });
    if (page.ready) owner.props.current.onData(page.ready);
    return () => {
      owner.props.current.nativeRef.current = null;
      page.owner = undefined;
      this.publish(this.pages.filter((other) => other !== page));
    };
  }
}

const Pages = createContext<EditorPages | null>(null);
const parked = 'leetcode-editor-standby';
const fill = { flex: 1 };

function PhysicalPage({ source, bind, onData, onStart, beforeLoad, backgroundColor, canvas, onTerminate }: {
  source: Source; bind: (view: WebView | null) => void; onData: (raw: string) => void;
  onStart: () => void; onTerminate: () => void; beforeLoad?: string; backgroundColor?: string; canvas: boolean;
}) {
  return <WebView<object> testID="CodeEditorInput" ref={bind} source={source}
    preloadOnMount={Platform.OS === 'ios' && canvas}
    injectedJavaScriptBeforeContentLoaded={beforeLoad}
    onMessage={(event) => onData(event.nativeEvent.data)}
    onContentProcessDidTerminate={onTerminate}
    onLoadStart={onStart} onLoadEnd={() => Logger.info('editor.webview.load_end', {})}
    onError={(e) => Logger.warn('editor.webview.error', { desc: e.nativeEvent.description })}
    onHttpError={(e) => Logger.warn('editor.webview.http_error', { statusCode: e.nativeEvent.statusCode })}
    onRenderProcessGone={(e) => Logger.error('editor.webview.process_gone', { didCrash: e.nativeEvent.didCrash })}
    originWhitelist={['*']} javaScriptEnabled domStorageEnabled={false} bounces={false} scrollEnabled={false}
    keyboardDisplayRequiresUserAction={false} hideKeyboardAccessoryView allowsInlineMediaPlayback
    style={{ flex: 1, backgroundColor }} />;
}

function ResidentPage({ page, source }: { page: Page; source: Source }) {
  const bind = useCallback((view: WebView | null) => {
    page.native = view;
    if (page.owner) page.owner.props.current.nativeRef.current = view;
  }, [page]);
  return <Portal hostName={page.owner?.host ?? parked} style={StyleSheet.absoluteFill}>
    <PhysicalPage source={source} bind={bind} canvas backgroundColor={page.owner?.props.current.backgroundColor}
      onStart={() => {
        page.ready = undefined;
        page.owner?.props.current.onLoadStart();
      }}
      onTerminate={() => {
        page.ready = undefined;
        if (page.owner) page.owner.props.current.onTerminate();
        else page.native?.reload();
      }}
      onData={(raw) => {
        if (page.owner) {
          page.owner.props.current.onData(raw);
          return;
        }
        try {
          if (JSON.parse(raw)?.type === 'ready') page.ready = raw;
        } catch (error) {
          Logger.withError(error).debug('editor.message.parse_skip');
        }
      }} />
  </Portal>;
}

function ResidentPages({ pages }: { pages: EditorPages }) {
  const snapshot = useSyncExternalStore(pages.subscribe, pages.snapshot);
  return <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={[StyleSheet.absoluteFill, { opacity: 0 }]}>
    <PortalHost name={parked} style={StyleSheet.absoluteFill} />
    {snapshot.map((page) => <ResidentPage key={page.id} page={page} source={pages.source} />)}
  </View>;
}

function NativePages({ children, getSource }: { children: ReactNode; getSource: () => Source }) {
  const [pages] = useState(() => new EditorPages(getSource()));
  return <PortalProvider><Pages.Provider value={pages}>
    {children}<ResidentPages pages={pages} />
  </Pages.Provider></PortalProvider>;
}

export function PreparedEditorPages({ children, getSource }: { children: ReactNode; getSource: () => Source }) {
  if (Platform.OS === 'ios' && UIManager.hasViewManagerConfig('PortalHostView')) {
    return <NativePages getSource={getSource}>{children}</NativePages>;
  }
  return <>{children}</>;
}

export function EditorPage(props: PageProps) {
  const pages = useContext(Pages);
  const host = useId();
  const latest = useRef(props);
  latest.current = props;
  const { source, nativeRef } = props;
  const resident = props.canvas && pages && ('uri' in source && 'uri' in pages.source
    ? source.uri === pages.source.uri : 'html' in source && 'html' in pages.source && source.html === pages.source.html);
  useLayoutEffect(() => {
    if (resident && pages) return pages.attach({ host, props: latest });
  }, [host, resident, pages]);
  const bind = useCallback((view: WebView | null) => { nativeRef.current = view; }, [nativeRef]);
  if (resident) return <PortalHost name={host} style={fill} />;
  return <PhysicalPage {...props} bind={bind} onStart={props.onLoadStart} />;
}
