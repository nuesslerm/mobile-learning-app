import { useRouteNavigation } from './useRouteAction';
import type { Blind75Problem, Difficulty } from '../domain';
import { Logger } from '@lib/logging';
import { useSkin } from '@lib/skin';
import { Badge, Button, IconButton, InsetGroup, ListItem } from '@lib/tamagui-design-system';
import { CollapsingHeader } from '@lib/tamagui-design-system/composites/collapsing-header';
import { useNavigation } from 'expo-router';
import { Menu } from '@lib/tamagui-design-system/icons';
import { Fragment, useCallback, useMemo } from 'react';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { ProblemCatalog } from '../domain/problemCatalog';
import { useLeetCodeAuthStore } from '../state/authStore';
import { useLeetCodeFlag } from '../state/useLeetCodeFlag';
import { AutocompleteSettingRow } from './AutocompleteSettingRow';
import { BlockViewSettingRow } from './BlockViewSettingRow';
import { CursorHandleSettingRow } from './editor/CursorHandleSettingRow';
import { RunnerSettingRow } from './RunnerSettingRow';

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

function AuthRow({ pushRoute }: { pushRoute: ReturnType<typeof useRouteNavigation>['pushRoute'] }) {
  const isLoggedIn = useLeetCodeAuthStore((s) => s.isLoggedIn);
  const username = useLeetCodeAuthStore((s) => s.username);
  const clearCredentials = useLeetCodeAuthStore((s) => s.clearCredentials);
  const { Row, Stack, Text } = useSkin();

  return (
    <Row
      align="center"
      justify="between"
      gap="md"
      marginTop="md"
      marginX="lg"
      paddingX={14}
      paddingY="md"
      radius="md"
      bordered
      surface="base"
      testID="LeetcodeListAuthRow"
    >
      <Stack grow>
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
      </Stack>
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
            pushRoute('/leetcode/login');
          }}
        >
          Sign in
        </Button>
      )}
    </Row>
  );
}

function ProblemRow({ problem, pushRoute }: { problem: Blind75Problem; pushRoute: ReturnType<typeof useRouteNavigation>['pushRoute'] }) {
  const { Row, Text } = useSkin();
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
        pushRoute(`/leetcode/${problem.id}`);
      }}
    >
      <Row align="center" gap="md">
        <Text grow numberOfLines={1} testID="LeetcodeListItemTitle">
          {problem.leetcodeId}. {problem.title}
        </Text>
        <Badge
          variant={DIFFICULTY_VARIANT[problem.difficulty]}
          testID="LeetcodeListItemDifficulty"
        >
          {problem.difficulty.toUpperCase()}
        </Badge>
      </Row>
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
  const { Stack, Text } = useSkin();
  return (
    <Stack paddingX={10} paddingY="xs" radius="pill" surface="onAccent" testID={testID}>
      <Text variant="small" tone="onAccent" testID={`${testID}Text`}>
        {children}
      </Text>
    </Stack>
  );
}

export function ProblemListScreen(): React.ReactElement {
  const { pushRoute } = useRouteNavigation();
  const insets = useSafeAreaInsets();
  const cursorPad = useLeetCodeFlag('cursorPad');
  const categories = useMemo(() => ProblemCatalog.getCategories(), []);
  const total = useMemo(() => ProblemCatalog.getAll().length, []);
  const navigation = useNavigation();
  const { Row, Stack, Text } = useSkin();

  // This screen sits one level inside the drawer's own Stack, so the Stack's
  // navigation parent is the Drawer navigator. `{ type: 'OPEN_DRAWER' }` is
  // the entire body of DrawerActions.openDrawer(), inlined to avoid pulling
  // @react-navigation/native in for one constant.
  const openDrawer = useCallback(() => {
    navigation.getParent()?.dispatch({ type: 'OPEN_DRAWER' });
  }, [navigation]);

  const expanded = (
    <Stack surface="accent" paddingX="lg" paddingBottom="xl" paddingTop={insets.top + 12} testID="LeetcodeListHero">
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="LeetcodeListMenuButton"
      />
      <Row align="center" gap="sm" marginTop="sm">
        <HeroChip testID="LeetcodeListHeroLive">🟢 LIVE</HeroChip>
        <HeroChip testID="LeetcodeListHeroCount">{total} PROBLEMS</HeroChip>
      </Row>
      <Text variant="title" tone="onAccent" marginTop="md" testID="LeetcodeListHeroTitle">
        Blind 75
      </Text>
      <Text tone="onAccent" dimmed marginTop="xs" testID="LeetcodeListHeroSubtitle">
        Master the patterns. Ace the interview. 🚀
      </Text>
    </Stack>
  );

  const compact = (
    <Row surface="accent" align="center" gap="sm" paddingX="lg" paddingBottom="md" paddingTop={insets.top + 6} testID="LeetcodeListCompactBar">
      <IconButton
        icon={Menu}
        variant="ghost"
        onPress={openDrawer}
        aria-label="Open menu"
        testID="LeetcodeListMenuButtonCompact"
      />
      <Text variant="large" tone="onAccent" grow numberOfLines={1} testID="LeetcodeListCompactTitle">
        Blind 75
      </Text>
      <HeroChip testID="LeetcodeListCompactCount">{total}</HeroChip>
    </Row>
  );

  return (
    <Stack grow surface="base" testID="LeetcodeListScreen">
      <CollapsingHeader
        expanded={expanded}
        compact={compact}
        contentContainerStyle={CONTENT_STYLE}
        testID="LeetcodeListHeader"
        scrollViewTestID="LeetcodeListScroll"
      >
        <AuthRow pushRoute={pushRoute} />
        <RunnerSettingRow />
        <AutocompleteSettingRow />
        {cursorPad ? <CursorHandleSettingRow /> : null}
        <BlockViewSettingRow />
        {categories.map((category) => (
          <Fragment key={category.id}>
            <Text variant="heading" marginTop="xl" marginBottom="sm" testID="LeetcodeListSectionTitle">
              {category.title}
            </Text>
            <InsetGroup testID="LeetcodeListSection">
              {category.problems.map((problem) => (
                <ProblemRow pushRoute={pushRoute} key={problem.id} problem={problem} />
              ))}
            </InsetGroup>
          </Fragment>
        ))}
      </CollapsingHeader>
    </Stack>
  );
}
