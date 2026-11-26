import { useEffect, useRef, useState, type ComponentRef } from 'react';
import {
  Button,
  Platform,
  Pressable,
  Text,
  TextInput,
  View,
} from 'react-native';
import {
  Maximize2,
  Minimize2,
  ChevronLeft,
  ChevronRight,
  Monitor,
  RefreshCw,
  Smartphone,
} from 'lucide-react-native';
import { BrowserViewport } from './BrowserViewport';
import { TabControl, useTabColors } from './TabControls';
import { Logger } from '@lib/logging';
import type { Pairing } from '../../domain/mobile';
import type {
  BrowserConnection,
  WorkspaceLayout,
  WorkspaceTab,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';

type Props = {
  api: WorkspaceTabsPort;
  pair: Pairing;
  layout: WorkspaceLayout;
  tab: WorkspaceTab;
  color: string;
  fullscreen: boolean;
  toggleFullscreen: () => void;
};
export function BrowserTab({
  api,
  pair,
  layout,
  tab,
  color,
  fullscreen,
  toggleFullscreen,
}: Props) {
  const [uri, frame] = useState('');
  const [address, setAddress] = useState('');
  const [controlsVisible, setControlsVisible] = useState(false);
  const hideControls = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined
  );
  const showControls = () => {
    clearTimeout(hideControls.current);
    setControlsVisible(true);
    hideControls.current = setTimeout(() => setControlsVisible(false), 3000);
  };
  useEffect(() => () => clearTimeout(hideControls.current), []);
  const [error, setError] = useState('');
  const [revision, retry] = useState(0);
  const c = useTabColors();
  const [mobile, setMobile] = useState(false);
  const [viewport, setViewport] = useState({ width: 390, height: 680 });
  const [history, setHistory] = useState({ back: false, forward: false });
  const keyboard = useRef<ComponentRef<typeof TextInput>>(null);
  const currentUrl = useRef('');
  const editing = useRef(false);
  const stream = useRef<BrowserConnection | null>(null);
  const size = useRef({ width: 1280, height: 800, mobile: false });
  const scale = mobile ? 1 : 1280 / Math.max(viewport.width, 1);
  useEffect(() => {
    size.current = {
      width: Math.round(viewport.width * scale),
      height: Math.min(4096, Math.round(viewport.height * scale)),
      mobile,
    };
    stream.current?.resize(size.current.width, size.current.height, mobile);
  }, [viewport, scale, mobile]);
  useEffect(() => {
    let live = true;
    setError('');
    frame('');
    api
      .browser(
        pair,
        layout,
        tab,
        (value) => {
          if (live) frame(value);
        },
        (msg) => {
          if (live) setError(msg);
        },
        (url, back, forward) => {
          if (!live) return;
          currentUrl.current = url;
          if (!editing.current) setAddress(url);
          setHistory({ back, forward });
        },
        Platform.OS === 'web'
          ? undefined
          : (editable) => {
              if (!live) return;
              if (editable) keyboard.current?.focus();
              else keyboard.current?.blur();
            }
      )
      .then((connection) => {
        if (!live) {
          connection.close();
          return;
        }
        stream.current = connection;
        connection.resize(
          size.current.width,
          size.current.height,
          size.current.mobile
        );
      })
      .catch((cause) => {
        Logger.withError(cause).warn('coding_agent.browser_connect_failed');
        if (live) setError(cause.message);
      });
    return () => {
      live = false;
      stream.current?.close();
      stream.current = null;
    };
  }, [api, pair, layout, tab, revision]);
  return (
    <View style={{ flex: 1 }}>
      {!fullscreen && (
        <View
          style={{
            flexDirection: 'row',
            alignItems: 'center',
            gap: 4,
            paddingHorizontal: 8,
            height: 34,
            borderBottomWidth: 1,
            borderColor: c.border,
            backgroundColor: c.bg,
          }}
        >
          <TabControl
            browser="tool"
            label="Back in browser"
            disabled={!history.back}
            onPress={() => stream.current?.history(-1)}
          >
            <ChevronLeft size={15} color={color} />
          </TabControl>
          <TabControl
            browser="tool"
            label="Forward in browser"
            disabled={!history.forward}
            onPress={() => stream.current?.history(1)}
          >
            <ChevronRight size={15} color={color} />
          </TabControl>
          <TabControl
            browser="tool"
            label="Reload"
            onPress={() => stream.current?.reload()}
          >
            <RefreshCw size={14} color={color} />
          </TabControl>
          <TextInput
            accessibilityLabel="Browser address"
            selectTextOnFocus
            style={{
              flex: 1,
              color,
              borderWidth: 1,
              borderColor: c.border,
              borderRadius: 12,
              paddingHorizontal: 9,
              paddingVertical: 2,
              fontSize: 14,
            }}
            placeholder="Enter URL"
            value={address}
            onChangeText={setAddress}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="url"
            returnKeyType="go"
            onFocus={() => {
              editing.current = true;
            }}
            onBlur={() => {
              editing.current = false;
              setAddress(currentUrl.current);
            }}
            onSubmitEditing={(e) => {
              const raw = e.nativeEvent.text.trim();
              if (raw)
                stream.current?.navigate(
                  /^https?:\/\//i.test(raw) ? raw : 'https://' + raw
                );
            }}
          />
          <TabControl
            browser="tool"
            label="Show web website view"
            selected={!mobile}
            onPress={() => setMobile(false)}
          >
            <Monitor size={14} color={!mobile ? c.bg : color} />
          </TabControl>
          <TabControl
            browser="tool"
            label="Show mobile website view"
            selected={mobile}
            onPress={() => setMobile(true)}
          >
            <Smartphone size={14} color={mobile ? c.bg : color} />
          </TabControl>
        </View>
      )}
      <View style={{ flex: 1 }}>
        {Platform.OS !== 'web' && (
          <TextInput
            ref={keyboard}
            accessible={false}
            tabIndex={-1}
            value=""
            autoCorrect={false}
            autoCapitalize="none"
            caretHidden
            style={{ position: 'absolute', width: 1, height: 1, opacity: 0 }}
            onChangeText={(text) => {
              if (text) stream.current?.text(text);
            }}
            onKeyPress={({ nativeEvent }) => {
              if (nativeEvent.key === 'Backspace') stream.current?.key('Backspace');
            }}
            onSubmitEditing={() => stream.current?.key('Enter')}
            submitBehavior="submit"
          />
        )}
        <BrowserViewport
          key={String(mobile)}
          onInteraction={showControls}
          uri={uri}
          viewport={viewport}
          onLayout={setViewport}
          click={(x, y, right) => {
            stream.current?.click(
              x * scale,
              y * scale,
              [],
              right ? 'right' : 'left'
            );
          }}
          wheel={(dx, dy) => stream.current?.wheel(dx * scale, dy * scale)}
        />
        {controlsVisible && (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={
              fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'
            }
            onPress={() => {
              toggleFullscreen();
              showControls();
            }}
            style={{
              position: 'absolute',
              right: 12,
              bottom: 12,
              width: 44,
              height: 44,
              borderRadius: 22,
              backgroundColor: 'rgba(0,0,0,0.74)',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            {fullscreen ? (
              <Minimize2 color="white" size={19} />
            ) : (
              <Maximize2 color="white" size={19} />
            )}
          </Pressable>
        )}
      </View>
      {!!error && (
        <>
          <Text accessibilityRole="alert" style={{ color }}>
            {error}
          </Text>
          <Button
            title="Reconnect browser"
            onPress={() => retry((n) => n + 1)}
          />
        </>
      )}
    </View>
  );
}
