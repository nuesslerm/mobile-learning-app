import {
  lazy,
  Suspense,
  useEffect,
  useRef,
  useState,
  type ComponentRef,
} from 'react';
import {
  Button,
  Keyboard,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
} from 'react-native';
import type { Pairing } from '../../domain/mobile';
import type {
  TerminalConnection,
  WorkspaceLayout,
  WorkspaceTab,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { Logger } from '@lib/logging';

import { TerminalMedia } from './TerminalMedia';
import { Keyboard as KeyboardIcon, Monitor } from 'lucide-react-native';
import { TabControl, useTabColors } from './TabControls';

const TerminalScreen = lazy(() => import('./TerminalScreen'));

type Props = {
  api: WorkspaceTabsPort;
  pair: Pairing;
  layout: WorkspaceLayout;
  tab: WorkspaceTab;
};
export function TerminalTab({ api, pair, layout, tab }: Props) {
  const [buffer, setBuffer] = useState('');
  const [snapshot, setSnapshot] = useState(0);
  const [size, setSize] = useState({ cols: 80, rows: 24 });
  const [command, setCommand] = useState('');
  const [error, setError] = useState('');
  const [ready, setReady] = useState(false);
  const [revision, retry] = useState(0);
  const stream = useRef<TerminalConnection | null>(null);
  const dimensions = useRef({ cols: 80, rows: 24 });

  useEffect(() => {
    let live = true;
    setReady(false);
    setError('');
    api
      .terminal(
        pair,
        layout,
        tab,
        (s, reset) => {
          if (!live) return;
          setReady(true);
          if (reset) setSnapshot((previous) => previous + 1);
          setBuffer((prev) => ((reset ? '' : prev) + s).slice(-200000));
        },
        (message) => {
          if (live) {
            setError(message);
            setReady(false);
          }
        }
      )
      .then((connection) => {
        if (!live) {
          connection.close();
          return;
        }
        stream.current = connection;
        connection.resize(dimensions.current.cols, dimensions.current.rows);
      })
      .catch((cause) => {
        Logger.withError(cause).warn('coding_agent.terminal_connect_failed');
        if (live) setError(cause.message);
      });
    return () => {
      live = false;
      stream.current?.close();
      stream.current = null;
    };
  }, [api, pair, layout, tab, revision]);
  const c = useTabColors();
  const input = useRef<ComponentRef<typeof TextInput>>(null);
  const [mediaError, setMediaError] = useState('');
  const [focused, setFocused] = useState(false);
  const [fit, setFit] = useState(true);
  const [width, setWidth] = useState(440);
  const pending = useRef('');
  const fontSize = fit ? Math.min(13, width / (80 * 0.61)) : 13;
  const lineHeight = Math.ceil(fontSize * 1.29);
  const heightRef = useRef(0);
  useEffect(() => {
    if (!heightRef.current) return;
    const cols = Math.max(10, Math.floor(width / (fontSize * 0.61)));
    const rows = Math.max(2, Math.floor(heightRef.current / lineHeight));
    dimensions.current = { cols, rows };
    setSize(dimensions.current);
    stream.current?.resize(cols, rows);
  }, [width, fontSize, lineHeight]);
  function type(value: string) {
    const old = pending.current;
    let prefix = 0;
    while (
      prefix < old.length &&
      prefix < value.length &&
      old[prefix] === value[prefix]
    )
      prefix++;
    const bytes =
      '\x7f'.repeat(Array.from(old.slice(prefix)).length) + value.slice(prefix);
    if (bytes) stream.current?.write(bytes);
    pending.current = value;
    setCommand(value);
  }
  function submit() {
    if (ready) {
      stream.current?.write('\r');
      pending.current = '';
      setCommand('');
    }
  }

  return (
    <View style={{ flex: 1, backgroundColor: '#191b26' }}>
      <View
        style={{ flex: 1 }}
        onLayout={(e) => {
          const { width, height } = e.nativeEvent.layout;
          heightRef.current = height;
          setWidth(width);
          dimensions.current = {
            cols: Math.max(10, Math.floor(width / (fontSize * 0.61))),
            rows: Math.max(2, Math.floor(height / lineHeight)),
          };
          setSize(dimensions.current);
          stream.current?.resize(
            dimensions.current.cols,
            dimensions.current.rows
          );
        }}
      >
        <Suspense fallback={null}>
          <TerminalScreen
            output={buffer}
            snapshot={snapshot}
            cols={size.cols}
            rows={size.rows}
            fontSize={fontSize}
            dom={{
              useExpoDOMWebView: false,
              scrollEnabled: false,
              style: { flex: 1, backgroundColor: '#191b26' },
            }}
          />
        </Suspense>
      </View>
      {!!error && (
        <>
          <Text accessibilityRole="alert" style={{ color: 'white' }}>
            {error}
          </Text>
          <Button title="Reconnect" onPress={() => retry((n) => n + 1)} />
        </>
      )}
      <View
        style={{
          backgroundColor: c.bg,
          paddingBottom: 8,
        }}
      >
        <ScrollView
          horizontal
          showsHorizontalScrollIndicator={false}
          keyboardShouldPersistTaps="always"
          style={{ flexGrow: 0, height: 46 }}
          contentContainerStyle={{
            paddingHorizontal: 8,
            gap: 8,
            alignItems: 'center',
          }}
        >
          <TabControl
            terminal
            label="Toggle keyboard"
            onPress={() =>
              focused ? Keyboard.dismiss() : input.current?.focus()
            }
          >
            <KeyboardIcon size={17} color={c.ink} />
          </TabControl>
          <TabControl
            terminal
            label="Fit terminal to screen"
            onPress={() => setFit((v) => !v)}
          >
            <Monitor size={16} color={c.ink} />
          </TabControl>
          <TabControl
            terminal
            label="Paste"
            onPress={() =>
              void import('expo-clipboard')
                .then((m) => m.getStringAsync())
                .then((value) => {
                  if (value) stream.current?.write(value);
                })
                .catch((e) =>
                  Logger.withError(e).warn('coding_agent.terminal_paste_failed')
                )
            }
          />
          {Object.entries({
            Esc: '\x1b',
            Tab: '\t',
            Enter: '\r',
            'Shift+Tab': '\x1b[Z',
            'Ctrl-C': '\x03',
            'Ctrl-D': '\x04',
            '↑': '\x1b[A',
            '↓': '\x1b[B',
            '←': '\x1b[D',
            '→': '\x1b[C',
          }).map(([label, bytes]) => (
            <TabControl
              terminal
              key={label}
              label={label}
              disabled={!ready}
              onPress={() => {
                if (label === 'Enter') submit();
                else {
                  stream.current?.write(bytes);
                  if (label === 'Ctrl-C' || label === 'Ctrl-D') {
                    pending.current = '';
                    setCommand('');
                  }
                }
              }}
            />
          ))}
        </ScrollView>
        {!!mediaError && (
          <Text accessibilityRole="alert" style={{ color: c.ink }}>
            {mediaError}
          </Text>
        )}
        <View
          style={{
            flexDirection: 'row',
            gap: 8,
            alignItems: 'center',
            paddingHorizontal: 12,
            paddingTop: 8,
          }}
        >
          <Pressable
            accessible={false}
            disabled={!ready}
            onPress={() => input.current?.focus()}
            style={{
              flex: 1,
              flexDirection: 'row',
              alignItems: 'center',
              gap: 8,
              borderWidth: focused ? 2 : 1,
              borderColor: focused ? '#3875ff' : c.border,
              borderRadius: 12,
              minHeight: 46,
              paddingHorizontal: 10,
            }}
          >
            <KeyboardIcon size={16} color={c.muted} />
            <View style={{ flex: 1 }}>
              <Text style={{ fontSize: 10, fontWeight: '600', color: c.muted }}>
                Live input
              </Text>
              <TextInput
                ref={input}
                accessibilityLabel="Terminal input"
                value={command}
                editable={ready}
                autoCapitalize="none"
                autoCorrect={false}
                spellCheck={false}
                keyboardType="ascii-capable"
                style={{
                  outlineStyle: 'solid',
                  outlineWidth: 0,
                  padding: 0,
                  fontSize: 14,
                  fontFamily: 'Menlo',
                  color: c.ink,
                }}
                placeholderTextColor={c.muted}
                placeholder={
                  ready
                    ? 'Tap to show keyboard'
                    : error
                      ? 'Terminal disconnected'
                      : 'Connecting…'
                }
                onFocus={() => setFocused(true)}
                onBlur={() => setFocused(false)}
                onChangeText={type}
                onSubmitEditing={submit}
                blurOnSubmit={false}
                submitBehavior="submit"
              />
            </View>
          </Pressable>
          <TerminalMedia
            pair={pair}
            checkout={layout.checkout}
            api={api}
            editable={ready}
            insert={(value) => {
              setMediaError('');
              type(pending.current + value);
            }}
            notify={setMediaError}
          />
        </View>
      </View>
    </View>
  );
}
