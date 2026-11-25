import { useAgentComponents } from '@lib/agent-ui-contracts';
import { router } from 'expo-router';
import { useState } from 'react';
import { Text, View } from '@lib/tamagui-design-system';

import { useCodingAgentPorts } from '../../state/ports';
import { PairDeviceForm } from '../PairDeviceForm';
import { UnpairedFeed } from '../UnpairedFeed';

/**
 * The staged home's feed: the unpaired feed, and the pairing it leads to.
 *
 * The pairing is the coding agent's own: the same form, the same port, the
 * same device. Once the device is paired, the coding agent's home takes over,
 * because the staged home carries the unpaired one.
 */
export function OrcaFeedTab() {
  const { HomeTabHeader } = useAgentComponents();
  const { mobile } = useCodingAgentPorts();
  const [pairing, setPairing] = useState(false);
  const [error, setError] = useState('');

  if (!pairing) {
    return <UnpairedFeed onPairDesktop={() => setPairing(true)} />;
  }

  return (
    <View flex={1}>
      <HomeTabHeader
        title="Pair desktop"
        actions={[
          {
            id: 'back',
            accessibilityLabel: 'Back to feed',
            icon: 'back',
            onPress: () => setPairing(false),
          },
        ]}
      />
      <View px="$4" py="$3" gap="$3">
        {error ? (
          <Text color="$red10" accessibilityRole="alert">
            {error}
          </Text>
        ) : null}
        <PairDeviceForm
          onPair={async (endpoint, code) => {
            if (!mobile) return false;
            setError('');
            try {
              await mobile.pair(endpoint, code);
              router.replace('/coding-agent');
              return true;
            } catch (failure) {
              setError(String(failure));
              return false;
            }
          }}
        />
      </View>
    </View>
  );
}
