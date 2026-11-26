import { terminalText } from '../../domain/workspaceTabs/terminalText';
import { useEffect, useRef, useState, type ComponentRef } from 'react';
import { Button, ScrollView, Text, TextInput, View } from 'react-native';
import type { Pairing } from '../../domain/mobile';
import type {
  TerminalConnection,
  WorkspaceLayout,
  WorkspaceTab,
  WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { Logger } from '@lib/logging';

type Props = {
  api: WorkspaceTabsPort;
  pair: Pairing;
  layout: WorkspaceLayout;
  tab: WorkspaceTab;
};
export function TerminalTab({ api, pair, layout, tab }: Props) {
  const [buffer, setBuffer] = useState('');
  const [command, setCommand] = useState('');
  const [error, setError] = useState('');
  const [ready, setReady] = useState(false);
  const [revision, retry] = useState(0);
  const stream = useRef<TerminalConnection | null>(null);
  const dimensions = useRef({ cols: 80, rows: 24 });
  const scroll = useRef<ComponentRef<typeof ScrollView>>(null);
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
  function submit() {
    if (ready) {
      stream.current?.write(command + '\r');
      setCommand('');
    }
  }
  const text = terminalText(buffer);
  return (
    <View style={{ flex: 1, backgroundColor: '#191b26', padding: 6 }}>
      <ScrollView
        ref={scroll}
        style={{ flex: 1 }}
        onContentSizeChange={() =>
          scroll.current?.scrollToEnd({ animated: false })
        }
        onLayout={(e) => {
          const { width, height } = e.nativeEvent.layout;
          dimensions.current = {
            cols: Math.max(10, Math.floor(width / 8.4)),
            rows: Math.max(2, Math.floor(height / 18)),
          };
          stream.current?.resize(
            dimensions.current.cols,
            dimensions.current.rows
          );
        }}
      >
        <Text
          selectable
          style={{
            fontFamily: 'Menlo',
            fontSize: 14,
            lineHeight: 18,
            color: '#e1e4ed',
          }}
        >
          {text}
        </Text>
      </ScrollView>
      {!!error && (
        <>
          <Text accessibilityRole="alert" style={{ color: 'white' }}>
            {error}
          </Text>
          <Button title="Reconnect" onPress={() => retry((n) => n + 1)} />
        </>
      )}
      <View style={{ flexDirection: 'row', alignItems: 'center' }}>
        <Button
          title="Ctrl-C"
          disabled={!ready}
          onPress={() => stream.current?.write('\u0003')}
        />
        <TextInput
          accessibilityLabel="Terminal input"
          value={command}
          onChangeText={setCommand}
          editable={ready}
          autoCapitalize="none"
          autoCorrect={false}
          style={{ flex: 1, padding: 10, color: 'white' }}
          placeholderTextColor="#aaaaaa"
          placeholder="Live input"
          onSubmitEditing={submit}
          blurOnSubmit={false}
        />
        <Button title="Enter" onPress={submit} disabled={!ready} />
      </View>
    </View>
  );
}
