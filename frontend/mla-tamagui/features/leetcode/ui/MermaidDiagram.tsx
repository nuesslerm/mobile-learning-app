import { Logger } from '@lib/logging';
import { Spinner, View } from '@lib/tamagui-design-system';
import { useRef, useState } from 'react';
import { StyleSheet, useColorScheme } from 'react-native';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';

type Props = {
  code: string;
  problemId?: string;
};

const MIN_HEIGHT = 80;

function buildHtml(code: string, theme: 'default' | 'dark'): string {
  const escaped = code
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  return `<!DOCTYPE html>
<html>
<head>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>
    body {
      margin: 0;
      padding: 8px;
      background: transparent;
      font-family: -apple-system, sans-serif;
    }
    .mermaid { width: 100%; }
    svg { max-width: 100% !important; }
  </style>
</head>
<body>
  <div class="mermaid">${escaped}</div>
  <script src="https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js"></script>
  <script>
    if (typeof mermaid === 'undefined') {
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'error', value: 'mermaid script not loaded' }));
    } else {
      mermaid.initialize({ startOnLoad: true, theme: '${theme}', securityLevel: 'loose' });
      mermaid.run().then(function() {
        var h = document.body.scrollHeight;
        window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'height', value: h }));
      }).catch(function(err) {
        window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'error', value: err.toString() }));
      });
    }
  </script>
</body>
</html>`;
}

export function MermaidDiagram({ code, problemId }: Props) {
  const colorScheme = useColorScheme();
  const theme = colorScheme === 'dark' ? 'dark' : 'default';
  const [height, setHeight] = useState(MIN_HEIGHT);
  const [loading, setLoading] = useState(true);
  const webViewRef = useRef<WebView>(null);

  const html = buildHtml(code.trim(), theme);

  const handleMessage = (event: WebViewMessageEvent) => {
    try {
      const parsed: unknown = JSON.parse(event.nativeEvent.data);
      if (
        parsed !== null &&
        typeof parsed === 'object' &&
        'type' in parsed &&
        'value' in parsed
      ) {
        const rec = parsed as Record<string, unknown>;
        if (rec['type'] === 'height' && typeof rec['value'] === 'number') {
          setHeight(Math.max(MIN_HEIGHT, rec['value']));
          setLoading(false);
        } else if (rec['type'] === 'error') {
          Logger.warn('leetcode.diagram.render_failed', {
            problemId,
            message: String(rec['value']).slice(0, 200),
            codeLength: code.length,
          });
          setLoading(false);
        }
      }
    } catch (err) {
      Logger.withError(err).warn('mermaidDiagram.message_parse_failed');
      setLoading(false);
    }
  };

  return (
    <View height={height} position="relative" testID="MermaidDiagram">
      {loading ? (
        <Spinner style={StyleSheet.absoluteFill} size="small" />
      ) : null}
      <WebView
        ref={webViewRef}
        source={{ html }}
        style={{ flex: 1, backgroundColor: 'transparent' }}
        scrollEnabled={false}
        onMessage={handleMessage}
        onError={(e) => {
          Logger.warn('leetcode.diagram.webview_error', {
            problemId,
            desc: e.nativeEvent.description,
          });
        }}
        onHttpError={(e) => {
          Logger.warn('leetcode.diagram.webview_error', {
            problemId,
            statusCode: e.nativeEvent.statusCode,
          });
        }}
        originWhitelist={['*']}
        javaScriptEnabled
      />
    </View>
  );
}
