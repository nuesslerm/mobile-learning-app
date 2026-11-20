import {
  createContext,
  useContext,
  useEffect,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import { View } from 'react-native';
import { WebView } from 'react-native-webview';

import type { CodeRunnerPort } from '../../domain/ports';
import { createExpoAssetIo } from './expoAssetIo';
import { PYODIDE_PAGE_HTML } from './pageHtml';
import type { PyodideFileName } from './pageHandlers';
import type { AssetIo } from './pyodideAssets';
import { createPyodideRunner } from './pyodideRunner';
import { createWebViewPageHost } from './webViewPageHost';

const RunnerContext = createContext<CodeRunnerPort | undefined>(undefined);

/** The runner the nearest provider holds, or none: a tree without the provider (a flow that mounts a route) gets an unavailable device runner. */
export function useOptionalPyodideCodeRunner(): CodeRunnerPort | undefined {
  return useContext(RunnerContext);
}

type Props = {
  children: ReactNode;
  /** the runtime assets as Metro modules; loaded when a run first needs them, so a user of the backend runner pays nothing */
  modules?: () => Promise<Record<PyodideFileName, number>>;
  pageHtml?: string;
  io?: AssetIo;
};

const defaultModules = async () =>
  (await import('./assets')).PYODIDE_ASSET_MODULES;

/**
 * The on-device runner for the whole app: mounted at the root, so the interpreter survives going from the list to a problem and back.
 * Its hidden WebView appears only when a run or a warm-up first needs the page, and draws nothing.
 */
export function PyodideRunnerProvider({
  children,
  modules = defaultModules,
  pageHtml = PYODIDE_PAGE_HTML,
  io,
}: Props) {
  const [{ view, runner }] = useState(() => {
    const created = createWebViewPageHost({
      modules,
      pageHtml,
      io: io ?? createExpoAssetIo(),
    });
    return {
      view: created.view,
      runner: createPyodideRunner({ host: created.host }),
    };
  });
  useEffect(() => () => runner.dispose(), [runner]);
  const page = useSyncExternalStore(view.subscribe, view.snapshot, () => null);
  return (
    <RunnerContext.Provider value={runner}>
      {children}
      {page ? (
        // 1 x 1, transparent and deaf to touch: it can never take the touch responder from the editor beside it
        <View
          pointerEvents="none"
          style={{ position: 'absolute', width: 1, height: 1, opacity: 0 }}
        >
          <WebView<object>
            key={page.generation}
            ref={view.bind}
            source={
              'uri' in page.source
                ? { uri: page.source.uri }
                : { html: page.source.html }
            }
            allowFileAccess
            allowFileAccessFromFileURLs
            allowUniversalAccessFromFileURLs
            allowingReadAccessToURL={
              'directory' in page.source ? page.source.directory : undefined
            }
            javaScriptEnabled
            domStorageEnabled={false}
            mixedContentMode="always"
            originWhitelist={['*']}
            onMessage={(event) =>
              view.receive(page.generation, event.nativeEvent.data)
            }
            onError={(event) =>
              view.lost(page.generation, event.nativeEvent.description)
            }
            onHttpError={(event) =>
              view.lost(page.generation, `http ${event.nativeEvent.statusCode}`)
            }
            onRenderProcessGone={(event) =>
              view.lost(
                page.generation,
                event.nativeEvent.didCrash ? 'crashed' : 'killed'
              )
            }
            onContentProcessDidTerminate={() =>
              view.lost(page.generation, 'terminated')
            }
            testID="PyodideHostWebView"
          />
        </View>
      ) : null}
    </RunnerContext.Provider>
  );
}
