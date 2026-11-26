import { useEffect, useRef, useState } from 'react';
import { Button, Image, Pressable, Text, TextInput, View } from 'react-native';
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
  const stream = useRef<BrowserConnection | null>(null);
  const size = useRef({ width: 390, height: 680 });
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
        }
      )
      .then((connection) => {
        if (!live) {
          connection.close();
          return;
        }
        stream.current = connection;
        connection.resize(size.current.width, size.current.height);
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
      <View style={{ flexDirection: 'row', alignItems: 'center' }}>
        <Button
          title="‹"
          accessibilityLabel="Back in browser"
          onPress={() => stream.current?.history(-1)}
        />
        <Button
          title="›"
          accessibilityLabel="Forward in browser"
          onPress={() => stream.current?.history(1)}
        />
        <TextInput
          accessibilityLabel="Browser address"
          style={{ flex: 1, color, padding: 8 }}
          placeholder="Enter URL"
          value={address}
          onChangeText={setAddress}
          autoCapitalize="none"
          autoCorrect={false}
          onSubmitEditing={() => stream.current?.navigate(address)}
        />
      </View>
      <Pressable
        accessibilityLabel="Desktop browser viewport"
        style={{ flex: 1 }}
        onLayout={(e) => {
          const { width, height } = e.nativeEvent.layout;
          if (width < 1 || height < 1) return;
          size.current = {
            width: Math.round(width),
            height: Math.round(height),
          };
          stream.current?.resize(size.current.width, size.current.height);
        }}
        onPress={(e) =>
          stream.current?.click(
            e.nativeEvent.locationX,
            e.nativeEvent.locationY
          )
        }
      >
        {uri ? (
          <Image
            accessible={false}
            pointerEvents="none"
            style={{ width: '100%', height: '100%' }}
            resizeMode="stretch"
            source={{ uri }}
          />
        ) : null}
      </Pressable>
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
      <View style={{ flexDirection: 'row' }}>
        <TextInput
          accessibilityLabel="Type on page"
          placeholder="Type on page…"
          value={text}
          onChangeText={setText}
          style={{ flex: 1, color, padding: 10 }}
          onSubmitEditing={submit}
        />
        <Button title="Enter" onPress={submit} />
      </View>
    </View>
  );
}
