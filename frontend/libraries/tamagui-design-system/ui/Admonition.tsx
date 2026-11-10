/**
 * Admonition — compound component for contextual callouts (info, success,
 * warning, error). Uses a left border accent to communicate variant intent.
 *
 * Usage:
 *   <Admonition variant="info" testID="MyScreen-Admonition">
 *     <Admonition.Icon />
 *     <Admonition.Title>Heads up</Admonition.Title>
 *     <Admonition.Body>Body content here.</Admonition.Body>
 *     <Admonition.Action>
 *       <Button>Learn more</Button>
 *     </Admonition.Action>
 *   </Admonition>
 *
 * The variant intent that app #1
 * encoded as `border-l-primary` / `text-success` classNames is expressed as
 * the same semantic theme keys (`$primary`, `$success`, `$warning`,
 * `$destructive`) resolved from the transcribed preset themes.
 */
import type { LucideIcon } from 'lucide-react-native';
import * as React from 'react';

import { CircleCheck, CircleX, Info, TriangleAlert } from '../icons';
import { YStack, type ColorTokens, type YStackProps } from '../primitives';

import { Icon } from './icon';
import { Text } from './text';

export type AdmonitionVariant = 'info' | 'success' | 'warning' | 'error';

const variantColorToken: Record<AdmonitionVariant, ColorTokens> = {
  info: '$primary',
  success: '$success',
  warning: '$warning',
  error: '$destructive',
};

const variantIcon: Record<AdmonitionVariant, LucideIcon> = {
  info: Info,
  success: CircleCheck,
  warning: TriangleAlert,
  error: CircleX,
};

type AdmonitionContextValue = {
  variant: AdmonitionVariant;
};

const AdmonitionContext = React.createContext<AdmonitionContextValue | null>(
  null
);

function useAdmonitionContext(): AdmonitionContextValue {
  const ctx = React.useContext(AdmonitionContext);
  if (ctx == null) {
    throw new Error(
      'Admonition subcomponents must be rendered inside <Admonition>.'
    );
  }
  return ctx;
}

export type AdmonitionProps = YStackProps & {
  variant: AdmonitionVariant;
  children: React.ReactNode;
};

function AdmonitionRoot({
  variant,
  children,
  testID,
  ...props
}: AdmonitionProps) {
  const contextValue = React.useMemo(() => ({ variant }), [variant]);
  const resolvedTestID = testID ?? '__admonition_unnamed';

  return (
    <AdmonitionContext.Provider value={contextValue}>
      <YStack
        testID={resolvedTestID}
        gap="$2"
        rounded="$md"
        borderLeftWidth={4}
        borderLeftColor={variantColorToken[variant]}
        p="$4"
        bg="$card"
        {...props}
      >
        {children}
      </YStack>
    </AdmonitionContext.Provider>
  );
}

export type AdmonitionIconProps = {
  icon?: LucideIcon;
  size?: number;
};

function AdmonitionIcon({ icon, size = 18 }: AdmonitionIconProps) {
  const { variant } = useAdmonitionContext();
  const ResolvedIcon = icon ?? variantIcon[variant];
  return (
    <Icon as={ResolvedIcon} size={size} color={variantColorToken[variant]} />
  );
}

export type AdmonitionTitleProps = {
  children: React.ReactNode;
};

function AdmonitionTitle({ children }: AdmonitionTitleProps) {
  return (
    <Text testID="AdmonitionUi-Title" variant="strong">
      {children}
    </Text>
  );
}

export type AdmonitionBodyProps = {
  children: React.ReactNode;
};

function AdmonitionBody({ children }: AdmonitionBodyProps) {
  return (
    <Text testID="AdmonitionUi-Body" color="$muted-foreground" size="$base">
      {children}
    </Text>
  );
}

export type AdmonitionActionProps = {
  children: React.ReactNode;
};

function AdmonitionAction({ children }: AdmonitionActionProps) {
  return (
    <YStack mt="$1" testID="AdmonitionUi-Action">
      {children}
    </YStack>
  );
}

export const Admonition = Object.assign(AdmonitionRoot, {
  Icon: AdmonitionIcon,
  Title: AdmonitionTitle,
  Body: AdmonitionBody,
  Action: AdmonitionAction,
});
