import { useEffect, useRef, useState } from 'react';
import { Button, Text, TextInput, View } from 'react-native';
import {
  ArrowUp,
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
};
export function BrowserTab({ api, pair, layout, tab, color }: Props) {
  const [uri, frame] = useState('');
  const [address, setAddress] = useState('');
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const [revision, retry] = useState(0);
  const c = useTabColors();
  const [mobile, setMobile] = useState(false);
  const [viewport, setViewport] = useState({ width: 390, height: 680 });
  const [history, setHistory] = useState({ back: false, forward: false });
  const currentUrl = useRef('');
  const editing = useRef(false);
  const [modifiers, setModifiers] = useState<string[]>([]);
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
  const submit = () => {
    if (text) {
      stream.current?.text(text);
      setText('');
    } else stream.current?.key('Enter');
  };
  return (
    <View style={{ flex: 1 }}>
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
      <BrowserViewport
        key={String(mobile)}
        uri={uri}
        viewport={viewport}
        onLayout={setViewport}
        click={(x, y, right) => {
          stream.current?.click(
            x * scale,
            y * scale,
            modifiers,
            right ? 'right' : 'left'
          );
          setModifiers([]);
        }}
        wheel={(dx, dy) => stream.current?.wheel(dx * scale, dy * scale)}
      />
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
      <View
        style={{
          backgroundColor: c.bg,
          borderTopWidth: 1,
          borderColor: c.border,
          padding: 8,
          gap: 4,
        }}
      >
        <View style={{ flexDirection: 'row', gap: 4 }}>
          {['Cmd', 'Ctrl', 'Alt', 'Shift'].map((label) => (
            <TabControl
              key={label}
              browser="key"
              label={label}
              selected={modifiers.includes(label.toLowerCase())}
              onPress={() =>
                setModifiers((m) =>
                  m.includes(label.toLowerCase())
                    ? m.filter((k) => k !== label.toLowerCase())
                    : [...m, label.toLowerCase()]
                )
              }
            />
          ))}
        </View>
        <View style={{ flexDirection: 'row', gap: 4 }}>
          {['Enter', 'Backspace', 'Tab', 'Esc'].map((label) => (
            <TabControl
              key={label}
              browser="key"
              label={label}
              display={label === 'Backspace' ? '⌫' : label}
              onPress={() =>
                stream.current?.key(label === 'Esc' ? 'Escape' : label)
              }
            />
          ))}
        </View>
        <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          <TextInput
            accessibilityLabel="Type on page"
            placeholder="Type on page…"
            value={text}
            onChangeText={setText}
            autoCapitalize="none"
            autoCorrect={false}
            style={{
              flex: 1,
              color,
              borderWidth: 1,
              borderColor: c.border,
              borderRadius: 12,
              paddingHorizontal: 9,
              paddingVertical: 5,
              fontSize: 16,
              height: 34,
              fontFamily: 'Menlo',
            }}
            onSubmitEditing={submit}
          />
          <TabControl
            label="Send text to browser"
            disabled={!text}
            onPress={submit}
          >
            <ArrowUp size={18} color={color} />
          </TabControl>
        </View>
      </View>
    </View>
  );
}
