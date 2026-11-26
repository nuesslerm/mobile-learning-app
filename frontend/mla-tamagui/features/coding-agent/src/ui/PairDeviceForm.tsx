import { useState } from 'react';
import { Button, Text, TextField, View } from '@lib/tamagui-design-system';

/**
 * The pairing form of the coding agent.
 *
 * It asks for the desktop's address and the code the desktop issued, and hands
 * both to the caller: the screen that owns the form decides what pairing means
 * and where a paired device goes next. The code is cleared once the device is
 * paired, so a refused pairing keeps it for the next try.
 */
export function PairDeviceForm(props: {
  onPair: (endpoint: string, code: string) => Promise<boolean>;
}) {
  const [host, setHost] = useState('');
  const [code, setCode] = useState('');
  return (
    <View gap="$3">
      <Text variant="h3">Pair with RCA</Text>
      <TextField
        testID="MobilePairEndpoint"
        label="Server address"
        autoCapitalize="none"
        autoCorrect={false}
        value={host}
        onChangeText={setHost}
        placeholder="https://…"
      />
      <TextField
        testID="MobilePairCode"
        label="Pairing code"
        secureTextEntry
        autoCapitalize="none"
        value={code}
        onChangeText={setCode}
      />
      <Button
        testID="MobilePair"
        disabled={!host.trim() || !code.trim()}
        onPress={() => {
          void props.onPair(host, code).then((paired) => {
            if (paired) setCode('');
          });
        }}
      >
        Pair device
      </Button>
    </View>
  );
}
