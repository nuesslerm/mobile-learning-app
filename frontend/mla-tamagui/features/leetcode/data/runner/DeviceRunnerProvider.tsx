import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';

import type { CodeRunnerPort } from '../../domain/ports';
import { createJavaScriptRunner } from '../javascript/javascriptRunner';
import { createJavaScriptPageHost } from '../javascript/javascriptPageHost';
import { createExpoAssetIo } from '../pyodide/expoAssetIo';
import { PYODIDE_PAGE_HTML } from '../pyodide/pageHtml';
import type { PyodideFileName } from '../pyodide/pageHandlers';
import { resolvePage, type AssetIo } from '../pyodide/pyodideAssets';
import { createPyodideRunner } from '../pyodide/pyodideRunner';
import { createDeviceRunnerRouter } from './deviceRunnerRouter';
import { HiddenRunnerPage } from './HiddenRunnerPage';
import { createWebViewPageHost } from './webViewPageHost';

const RunnerContext = createContext<CodeRunnerPort | undefined>(undefined);

/** The runner the nearest provider holds, or none: a tree without the provider (a flow that mounts a route) gets an unavailable device runner. */
export function useOptionalDeviceCodeRunner(): CodeRunnerPort | undefined {
  return useContext(RunnerContext);
}

type Props = {
  children: ReactNode;
  /** the runtime assets as Metro modules; loaded when a run first needs them, so a user of the backend runner pays nothing */
  modules?: () => Promise<Record<PyodideFileName, number>>;
  pageHtml?: string;
  io?: AssetIo;
  /** the JavaScript runner's page, for a test that brings its own */
  javascriptPageHtml?: string;
};

const defaultModules = async () => (await import('../pyodide/assets')).PYODIDE_ASSET_MODULES;

/**
 * The on-device runner for the whole app: mounted at the root, so an interpreter survives going from the list to a problem and back.
 * One router answers for every language; each engine's hidden WebView appears only when a run or a warm-up first needs its page.
 */
export function DeviceRunnerProvider({ children, modules = defaultModules, pageHtml = PYODIDE_PAGE_HTML, io, javascriptPageHtml }: Props) {
  const [{ python, pythonView, javascript, javascriptView, router }] = useState(() => {
    const assetIo = io ?? createExpoAssetIo();
    const pythonPage = createWebViewPageHost({
      resolve: async () => resolvePage(await modules(), pageHtml, assetIo),
      chunks: assetIo.chunks,
    });
    const javascriptPage = createJavaScriptPageHost(javascriptPageHtml);
    const pythonRunner = createPyodideRunner({ host: pythonPage.host });
    const javascriptRunner = createJavaScriptRunner({ host: javascriptPage.host });
    return {
      python: pythonRunner,
      pythonView: pythonPage.view,
      javascript: javascriptRunner,
      javascriptView: javascriptPage.view,
      router: createDeviceRunnerRouter({ python3: pythonRunner, javascript: javascriptRunner }),
    };
  });
  useEffect(
    () => () => {
      python.dispose();
      javascript.dispose();
    },
    [python, javascript]
  );
  return (
    <RunnerContext.Provider value={router}>
      {children}
      <HiddenRunnerPage view={pythonView} testID="PyodideHostWebView" fileAccess />
      <HiddenRunnerPage view={javascriptView} testID="JavaScriptHostWebView" />
    </RunnerContext.Provider>
  );
}
