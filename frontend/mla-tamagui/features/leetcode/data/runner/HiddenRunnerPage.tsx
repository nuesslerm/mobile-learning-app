import { useSyncExternalStore } from 'react';
import { View } from 'react-native';
import { WebView } from 'react-native-webview';

import type { PageView } from './webViewPageHost';

/**
 * The hidden WebView of one on-device runner: it appears only while its host shows a page, and draws nothing. 1 x 1, transparent
 * and deaf to touch, so it can never take the touch responder from the editor beside it. `fileAccess` is for a page that reads
 * the app's bundled files (Pyodide's runtime); a page that needs none is given none.
 */
export function HiddenRunnerPage({ view, testID, fileAccess = false }: { view: PageView; testID: string; fileAccess?: boolean }) {
  const page = useSyncExternalStore(view.subscribe, view.snapshot, () => null);
  if (!page) return null;
  const { source } = page;
  return (
    <View pointerEvents="none" style={{ position: 'absolute', width: 1, height: 1, opacity: 0 }}>
      <WebView<object>
        key={page.generation}
        ref={view.bind}
        source={'uri' in source ? { uri: source.uri } : { html: source.html, ...(source.baseUrl ? { baseUrl: source.baseUrl } : {}) }}
        allowFileAccess={fileAccess}
        allowFileAccessFromFileURLs={fileAccess}
        allowUniversalAccessFromFileURLs={fileAccess}
        allowingReadAccessToURL={'directory' in source ? source.directory : undefined}
        javaScriptEnabled
        domStorageEnabled={false}
        mixedContentMode="always"
        originWhitelist={['*']}
        onMessage={(event) => view.receive(page.generation, event.nativeEvent.data)}
        onError={(event) => view.lost(page.generation, event.nativeEvent.description)}
        onHttpError={(event) => view.lost(page.generation, `http ${event.nativeEvent.statusCode}`)}
        onRenderProcessGone={(event) => view.lost(page.generation, event.nativeEvent.didCrash ? 'crashed' : 'killed')}
        onContentProcessDidTerminate={() => view.lost(page.generation, 'terminated')}
        testID={testID}
      />
    </View>
  );
}
