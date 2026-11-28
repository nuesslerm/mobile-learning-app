import { describe, it } from 'vitest';
import { RuleTester } from 'oxlint/plugins-dev';

import { noDesignSystemOutsideASkin, noReactNativeControls, onlyRootNamesASkin } from '../src/rules/skin-imports.js';

RuleTester.describe = describe;
RuleTester.it = it;

const ruleTester = new RuleTester();
type Tested = Parameters<typeof ruleTester.run>[1];

ruleTester.run('only-root-names-a-skin', onlyRootNamesASkin as unknown as Tested, {
  valid: [
    { code: "import { useSkin } from '@lib/skin';", filename: 'a.tsx' },
    { code: "import { thing } from '@lib/skinny';", filename: 'b.tsx' },
  ],
  invalid: [
    { code: "import { orcaSkin } from '@lib/orca-skin';", filename: 'c.tsx', errors: [{ messageId: 'onlyRootNamesASkin' }] },
    { code: "import { Row } from '@lib/tamagui-skin/src/adapters/Row';", filename: 'd.tsx', errors: [{ messageId: 'onlyRootNamesASkin' }] },
    { code: "export { tamaguiSkin } from '@lib/tamagui-skin';", filename: 'e.tsx', errors: [{ messageId: 'onlyRootNamesASkin' }] },
    { code: "import type { Anything } from '@lib/orca-skin';", filename: 'f.tsx', errors: [{ messageId: 'onlyRootNamesASkin' }] },
  ],
});

ruleTester.run('no-design-system-outside-a-skin', noDesignSystemOutsideASkin as unknown as Tested, {
  valid: [
    { code: "import { useSkin } from '@lib/skin';", filename: 'a.tsx' },
    { code: "import { View } from 'react-native';", filename: 'b.tsx' },
    { code: "import { x } from 'tamagui-like';", filename: 'c.tsx' },
  ],
  invalid: [
    { code: "import { Button } from '@lib/tamagui-design-system';", filename: 'd.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
    { code: "import { XStack } from '@lib/tamagui-design-system/primitives';", filename: 'e.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
    { code: "import { styled } from 'tamagui';", filename: 'f.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
    { code: "import { YStack } from '@tamagui/stacks';", filename: 'g.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
    { code: "import Check from 'lucide-react-native/dist/esm/icons/check.mjs';", filename: 'h.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
    { code: "export * from '@lib/tamagui-design-system/theme';", filename: 'i.tsx', errors: [{ messageId: 'noDesignSystemOutsideASkin' }] },
  ],
});

ruleTester.run('no-react-native-controls', noReactNativeControls as unknown as Tested, {
  valid: [
    { code: "import { View, ScrollView, FlatList, Pressable, Image, Platform, Keyboard } from 'react-native';", filename: 'a.tsx' },
    { code: "import type { TextInput, GestureResponderEvent } from 'react-native';", filename: 'b.tsx' },
    { code: "import { View, type TextInput } from 'react-native';", filename: 'c.tsx' },
    { code: "import { Text } from 'somewhere-else';", filename: 'd.tsx' },
  ],
  invalid: [
    { code: "import { Text } from 'react-native';", filename: 'e.tsx', errors: [{ messageId: 'noReactNativeControls' }] },
    { code: "import { Button, View, TextInput } from 'react-native';", filename: 'f.tsx', errors: [{ messageId: 'noReactNativeControls' }, { messageId: 'noReactNativeControls' }] },
    { code: "import { Switch as RNSwitch, ActivityIndicator, Modal, TouchableOpacity } from 'react-native';", filename: 'g.tsx', errors: [{ messageId: 'noReactNativeControls' }, { messageId: 'noReactNativeControls' }, { messageId: 'noReactNativeControls' }, { messageId: 'noReactNativeControls' }] },
  ],
});
