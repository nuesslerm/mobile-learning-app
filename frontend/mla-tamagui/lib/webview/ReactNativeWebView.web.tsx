import { Logger } from '@lib/logging';
import { View } from '@lib/tamagui-design-system';
import * as React from 'react';
import { type StyleProp, type ViewStyle } from 'react-native';

const BRIDGE_FLAG = '__RN_WEBVIEW_BRIDGE__';
const INJECT_FLAG = '__RN_WEBVIEW_INJECT__';

export type WebViewMessageEvent = {
  nativeEvent: {
    data: string;
  };
};

type WebViewSource = {
  html?: string;
  uri?: string;
};

export type WebViewHandle = {
  injectJavaScript: (script: string) => void;
  /** Blur the underlying iframe so document.activeElement is cleared. */
  blur: () => void;
};

type BridgePayload = {
  [BRIDGE_FLAG]?: unknown;
  data?: unknown;
};

function isBridgePayload(value: unknown): value is BridgePayload {
  return (
    typeof value === 'object' &&
    value !== null &&
    BRIDGE_FLAG in value &&
    Reflect.get(value, BRIDGE_FLAG) === true
  );
}

type WebViewProps = {
  source?: WebViewSource;
  onMessage?: (event: WebViewMessageEvent) => void;
  style?: StyleProp<ViewStyle>;
  testID?: string;
};

const BRIDGE_SCRIPT = `<script>(function(){window.ReactNativeWebView={postMessage:function(data){window.parent.postMessage({${JSON.stringify(
  BRIDGE_FLAG
)}:true,data:String(data)},'*');}};window.addEventListener('message',function(event){var payload=event.data||{};if(payload[${JSON.stringify(
  INJECT_FLAG
)}]===true){(0,eval)(String(payload.script||''));}});})();</script>`;
const FILL_FRAME = { width: '100%', height: '100%', borderWidth: 0 } as const;

function withBridge(html: string): string {
  if (html.includes(BRIDGE_FLAG)) return html;
  if (html.includes('</head>'))
    return html.replace('</head>', `${BRIDGE_SCRIPT}</head>`);
  return `${BRIDGE_SCRIPT}${html}`;
}

const WebView = React.forwardRef<WebViewHandle, WebViewProps>(
  function WebWebView({ source, onMessage, style, testID = 'WebView' }, ref) {
    const frameRef = React.useRef<HTMLIFrameElement | null>(null);

    React.useImperativeHandle(
      ref,
      () => ({
        injectJavaScript: (script: string) => {
          frameRef.current?.contentWindow?.postMessage(
            { [INJECT_FLAG]: true, script },
            '*'
          );
        },
        blur: () => {
          Logger.debug('webview.blur');
          const cw = frameRef.current?.contentWindow;
          if (cw?.document?.activeElement) {
            (cw.document.activeElement as HTMLElement).blur();
          }
          frameRef.current?.blur();
        },
      }),
      []
    );

    const onMessageRef = React.useRef(onMessage);
    onMessageRef.current = onMessage;

    // Attached before the iframe can load and never re-attached: a bridge
    // message arriving in an effect gap (the editor's single 'ready') was lost.
    React.useLayoutEffect(() => {
      const handleMessage = (event: MessageEvent) => {
        if (!isBridgePayload(event.data)) return;
        if (event.source !== frameRef.current?.contentWindow) return;
        onMessageRef.current?.({ nativeEvent: { data: String(event.data.data ?? '') } });
      };
      window.addEventListener('message', handleMessage);
      return () => {
        window.removeEventListener('message', handleMessage);
      };
    }, []);

    // A statically exported page renders the iframe before hydration; loading it
    // then would post 'ready' before any listener exists. Load only once mounted.
    const [mounted, setMounted] = React.useState(false);
    React.useLayoutEffect(() => {
      setMounted(true);
    }, []);
    const html = mounted && source?.html ? withBridge(source.html) : undefined;

    return (
      <View testID={testID} style={[{ overflow: 'hidden' }, style]}>
        <iframe
          ref={frameRef}
          src={mounted ? source?.uri : undefined}
          srcDoc={html}
          sandbox="allow-forms allow-same-origin allow-scripts"
          style={FILL_FRAME}
          title={testID ?? 'WebView'}
        />
      </View>
    );
  }
);

export default WebView;
export { WebView };
