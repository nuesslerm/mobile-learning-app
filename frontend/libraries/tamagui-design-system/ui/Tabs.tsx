import {
  Tabs as TamaguiTabs,
  useTabsContext,
  type TabsContentProps as TamaguiTabsContentProps,
  type TabsListProps as TamaguiTabsListProps,
  type TabsProps as TamaguiTabsRootProps,
  type TabsTabProps as TamaguiTabsTabProps,
} from '../primitives';

import { Text } from './text';

export type TabsProps = TamaguiTabsRootProps;
export type TabsListProps = TamaguiTabsListProps;
export type TabsTriggerProps = TamaguiTabsTabProps;
export type TabsContentProps = TamaguiTabsContentProps;

/**
 * Tabs on Tamagui's own Tabs primitive, a four-component surface (`Tabs`, `TabsList`, `TabsTrigger`,
 * `TabsContent`).
 *
 * App #1's tabs carry the overflow/scroll machinery and the 3D depth
 * underlay. Neither crosses over: overflow chips are app #1's answer to a
 * problem this port does not have yet, and depth is the grammar being
 * replaced. What survives is the shadcn look -- a muted rail, one
 * background-filled active trigger, muted-foreground inactive labels --
 * expressed as theme tokens only.
 */
export function Tabs(props: TabsProps) {
  return <TamaguiTabs gap="$2" {...props} />;
}

export function TabsList(props: TabsListProps) {
  return (
    <TamaguiTabs.List
      alignSelf="flex-start"
      bg="$color3"
      rounded="$md"
      p="$1"
      gap="$0.5"
      {...props}
    />
  );
}

export function TabsTrigger({ value, children, ...props }: TabsTriggerProps) {
  const { value: selectedValue } = useTabsContext();
  const isActive = value === selectedValue;

  return (
    <TamaguiTabs.Tab
      value={value}
      unstyled
      // $2.5 is 32px on the kept default size scale -- app #1's h-8.
      height="$2.5"
      px="$4"
      rounded="$sm"
      justify="center"
      items="center"
      bg={isActive ? '$background' : 'transparent'}
      pressStyle={{ opacity: 0.7 }}
      disabledStyle={{ opacity: 0.5 }}
      {...props}
    >
      <Text variant="small" color={isActive ? '$color' : '$color11'}>
        {children}
      </Text>
    </TamaguiTabs.Tab>
  );
}

export function TabsContent(props: TabsContentProps) {
  return <TamaguiTabs.Content flex={1} {...props} />;
}
