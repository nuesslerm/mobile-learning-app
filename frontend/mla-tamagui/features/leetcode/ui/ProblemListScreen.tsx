import type { Blind75Problem, Difficulty } from '../domain';
import { Logger } from '@lib/logging';
import { Badge, Button, IconButton, InsetGroup, ListItem, Text, View } from '@lib/tamagui-design-system';
import { CollapsingHeader } from '@lib/tamagui-design-system/composites/collapsing-header';
import { router, useNavigation } from 'expo-router';
import { Menu } from '@lib/tamagui-design-system/icons';
import { Fragment, useCallback, useMemo } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { ProblemCatalog } from '../data/problemCatalog';
import { useLeetCodeAuthStore } from '../state/authStore';

const CONTENT_STYLE = { paddingBottom: 24 } as const;

// A Badge variant, not a hex string. App #1 reads rgbaColors() out of a theme
// store and threads three colour strings down through three components; a
// variant carries the same meaning and follows light/dark on its own.
const DIFFICULTY_VARIANT: Record<
  Difficulty,
  'success' | 'warning' | 'destructive'
> = {
  easy: 'success',
  medium: 'warning',
  hard: 'destructive',
};

function AuthRow() {
  const isLoggedIn = useLeetCodeAuthStore((s) => s.isLoggedIn);
  const username = useLeetCodeAuthStore((s) => s.username);
  const clearCredentials = useLeetCodeAuthStore((s) => s.clearCredentials);

  return (
    <XStack
      items="center"
      justify="space-between"
      gap="$3"
      mt="$3"
      mx="$4"
      px="$3.5"
      py="$3"
      rounded="$4"
      borderWidth={1}
      borderColor="$borderColor"
      bg="$background"
      testID="LeetcodeListAuthRow"
    >
      <YStack flex={1}>
        <Text variant="muted" testID="LeetcodeListAuthLabel">
          {isLoggedIn ? 'Signed in as' : 'Not signed in'}
        </Text>
        <Text
          variant={isLoggedIn ? 'strong' : 'muted'}
          testID="LeetcodeListAuthValue"
        >
          {isLoggedIn
            ? username
            : 'Sign in to LeetCode to unlock account features.'}
        </Text>
      </YStack>
      {isLoggedIn ? (
        <Button
          variant="outline"
          size="$3"
          testID="LeetcodeListLogout"
          onPress={clearCredentials}
        >
          Log out
        </Button>
      ) : (
        <Button
          variant="default"
          size="$3"
          testID="LeetcodeListSignIn"
          pressLog="off"
          onPress={() => {
            Logger.info('leetcode.list.signin_tap', { isLoggedIn });
            router.push('/leetcode/login');
          }}
        >
          Sign in
        </Button>
      )}
    </XStack>
  );
}

function ProblemRow({ problem }: { problem: Blind75Problem }) {
  return (
    <ListItem
      testID="LeetcodeListItem"
      variant="grouped"
      showChevron
      pressLog="off"
      onPress={() => {
        Logger.info('leetcode.list.open_problem', {
          problemId: problem.id,
          leetcodeId: problem.leetcodeId,
        });
        router.push(`/leetcode/${problem.id}`);
      }}
    >
      <XStack items="center" gap="$3">
        <Text flex={1} numberOfLines={1} testID="LeetcodeListItemTitle">
          {problem.leetcodeId}. {problem.title}
        </Text>
        <Badge
          variant={DIFFICULTY_VARIANT[problem.difficulty]}
          testID="LeetcodeListItemDifficulty"
        >
          {problem.difficulty.toUpperCase()}
        </Badge>
      </XStack>
    </ListItem>
  );
}

function HeroChip({
  children,
  testID,
}: {
  children: React.ReactNode;
  testID: string;
}) {
  return (
    <YStack
      px="$2.5"
      py="$1"
      rounded="$10"
      bg="rgba(255,255,255,0.2)"
      testID={testID}
    >
      <Text variant="small" color="white" testID={`${testID}Text`}>
        {children}
      </Text>
    </YStack>
  );
}

export function ProblemListScreen(): React.ReactElement {
  const insets = useSafeAreaInsets();
  const categories = useMemo(() => ProblemCatalog.getCategories(), []);
  const total = useMemo(() => ProblemCatalog.getAll().length, []);
  const navigation = useNavigation();

  // This screen sits one level inside the drawer's own Stack, so the Stack's
  // navigation parent is the Drawer navigator. `{ type: 'OPEN_DRAWER' }` is
  // the entire body of DrawerActions.openDrawer(), inlined to avoid pulling
  // @react-navigation/native in for one constant.
  const openDrawer = useCallback(() => {
    navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' });
  }, [navigation]);

  const expanded = (
    <YStack
      bg="$blue9"
      px="$4"
      pb="$5"
      pt={insets.top + 12}
      testID="LeetcodeListHero"
    >
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="LeetcodeListMenuButton"
      />
      <XStack items="center" gap="$2" mt="$2">
        <HeroChip testID="LeetcodeListHeroLive">🟢 LIVE</HeroChip>
        <HeroChip testID="LeetcodeListHeroCount">{total} PROBLEMS</HeroChip>
      </XStack>
      <Text variant="h1" color="white" mt="$3" testID="LeetcodeListHeroTitle">
        Blind 75
      </Text>
      <Text
        color="white"
        opacity={0.85}
        mt="$1"
        testID="LeetcodeListHeroSubtitle"
      >
        Master the patterns. Ace the interview. 🚀
      </Text>
    </YStack>
  );

  const compact = (
    <XStack
      bg="$blue9"
      items="center"
      gap="$2"
      px="$4"
      pb="$3"
      pt={insets.top + 6}
      testID="LeetcodeListCompactBar"
    >
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="LeetcodeListMenuButtonCompact"
      />
      <Text
        variant="large"
        color="white"
        flex={1}
        numberOfLines={1}
        testID="LeetcodeListCompactTitle"
      >
        Blind 75
      </Text>
      <HeroChip testID="LeetcodeListCompactCount">{total}</HeroChip>
    </XStack>
  );

  return (
    <View flex={1} bg="$background" testID="LeetcodeListScreen">
      <CollapsingHeader
        expanded={expanded}
        compact={compact}
        contentContainerStyle={CONTENT_STYLE}
        testID="LeetcodeListHeader"
        scrollViewTestID="LeetcodeListScroll"
      >
        <AuthRow />
        {categories.map((category) => (
          <Fragment key={category.id}>
            <Text
              variant="h3"
              mt="$5"
              mb="$2"
              testID="LeetcodeListSectionTitle"
            >
              {category.title}
            </Text>
            <InsetGroup testID="LeetcodeListSection">
              {category.problems.map((problem) => (
                <ProblemRow key={problem.id} problem={problem} />
              ))}
            </InsetGroup>
          </Fragment>
        ))}
      </CollapsingHeader>
    </View>
  );
}
