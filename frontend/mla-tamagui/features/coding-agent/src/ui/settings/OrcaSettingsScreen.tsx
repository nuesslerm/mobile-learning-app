// The frame the coding agent's settings screens share: the skin's settings screen, with its way back.

import { useSkin } from '@lib/skin';
import { router } from 'expo-router';
import type { ReactNode } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function OrcaSettingsScreen(props: { title: string; children: ReactNode }) {
  const { HomeSettingsScreen } = useSkin();
  const { top } = useSafeAreaInsets();
  return (
    <HomeSettingsScreen title={props.title} topInset={top} onBack={() => router.canGoBack() ? router.back() : router.replace('/coding-agent')}>
      {props.children}
    </HomeSettingsScreen>
  );
}
