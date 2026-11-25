import { useAgentComponents } from '@lib/agent-ui-contracts';
import { useWindowDimensions } from 'react-native';

const steps = [
  {
    id: 'open-desktop',
    title: 'Open Orca desktop',
    description: 'Go to Settings → Mobile and generate a pairing QR code.',
  },
  {
    id: 'scan-code',
    title: 'Scan the code',
    description:
      'Tap Pair desktop below, then point your camera at the QR code on your screen.',
  },
  {
    id: 'connected',
    title: "You're connected",
    description:
      'Your desktop will appear here. Everything is encrypted end-to-end.',
  },
];

export function UnpairedFeed({ onPairDesktop }: { onPairDesktop: () => void }) {
  const { UnpairedHome } = useAgentComponents();
  const { width, height } = useWindowDimensions();
  return (
    <UnpairedHome
      contentMaxWidth={720}
      isWideLayout={width >= 700 && Math.min(width, height) >= 600}
      bottomInset={24}
      title="Connect your desktop"
      description="Pair with Orca on your computer to check on your agents, jump into any terminal, and drive work from your phone."
      steps={steps}
      pairLabel="Pair desktop"
      onPairDesktop={onPairDesktop}
    />
  );
}
