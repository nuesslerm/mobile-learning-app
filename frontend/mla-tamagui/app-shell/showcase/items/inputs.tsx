import {
  Chip,
  SegmentedControl,
  Switch,
  TextField,
} from '@lib/tamagui-design-system';
import { Code, Sparkles } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { Host, Picker, Switch as ExpoSwitch, TextInput } from '@expo/ui';
import { useState } from 'react';

import { Compare } from '../Compare';

export function TextFieldItem() {
  const [name, setName] = useState('');
  return (
    <Compare
      testID="ShowcaseTextField"
      tamagui={
        <TextField
          testID="ShowcaseNameField"
          label="Your name"
          placeholder="Ada Lovelace"
          value={name}
          onChangeText={setName}
          help="Rendered by the DS TextField"
        />
      }
      expoUi={
        <Host matchContents>
          <TextInput
            placeholder="Ada Lovelace"
            accessibilityLabel="Your name (Expo UI)"
          />
        </Host>
      }
    />
  );
}

export function SwitchItem() {
  const [on, setOn] = useState(true);
  const [expoOn, setExpoOn] = useState(true);
  return (
    <Compare
      testID="ShowcaseSwitch"
      tamagui={
        <Switch
          value={on}
          onValueChange={setOn}
          accessibilityLabel="Notifications"
          testID="ShowcaseSwitchControl"
        />
      }
      expoUi={
        <Host matchContents>
          <ExpoSwitch
            value={expoOn}
            onValueChange={setExpoOn}
            label="Notifications"
            testID="ShowcaseExpoSwitch"
          />
        </Host>
      }
    />
  );
}

export function SegmentedControlItem() {
  const [segment, setSegment] = useState<'build' | 'plan'>('build');
  return (
    <Compare
      testID="ShowcaseSegmented"
      tamagui={
        <SegmentedControl
          options={[
            { value: 'build', label: 'Build' },
            { value: 'plan', label: 'Plan' },
          ]}
          value={segment}
          onChange={(value) => setSegment(value as 'build' | 'plan')}
          testID="Showcase-CodingAgent-Segmented"
        />
      }
      expoUi={
        <Host matchContents>
          <Picker
            selectedValue={segment}
            onValueChange={(value) => setSegment(value as 'build' | 'plan')}
            accessibilityLabel="Mode"
            testID="ShowcaseExpoPicker"
          >
            <Picker.Item label="Build" value="build" />
            <Picker.Item label="Plan" value="plan" />
          </Picker>
        </Host>
      }
    />
  );
}

export function ChipItem() {
  const [mode, setMode] = useState<'build' | 'plan'>('build');
  return (
    <Compare
      testID="ShowcaseChip"
      tamagui={
        <XStack gap="$2" flexWrap="wrap">
          <Chip
            label="Build"
            icon={Code}
            selected={mode === 'build'}
            onPress={() => setMode('build')}
            testID="Showcase-CodingAgent-ChipBuild"
          />
          <Chip
            label="Plan"
            icon={Sparkles}
            selected={mode === 'plan'}
            onPress={() => setMode('plan')}
            testID="Showcase-CodingAgent-ChipPlan"
          />
          <Chip
            label="Disabled"
            icon={Code}
            selected={false}
            onPress={() => {}}
            testID="Showcase-CodingAgent-ChipDisabled"
          />
        </XStack>
      }
    />
  );
}
