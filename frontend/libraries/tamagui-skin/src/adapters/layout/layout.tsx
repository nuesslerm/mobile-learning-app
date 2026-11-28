import type { LayoutProps, Space, TextProps } from '@lib/skin';
import { Text as DesignText, type TextProps as DesignTextProps } from '@lib/tamagui-design-system';
import { XStack, YStack, type XStackProps } from '@lib/tamagui-design-system/primitives';

// The skin's scale in the design system's space tokens: 4, 8, 12, 16 and 20 points.
const SPACE = { none: 0, xs: '$1', sm: '$2', md: '$3', lg: '$4', xl: '$5' } as const;
const space = (value: Space | undefined) => (value === undefined || typeof value === 'number' ? value : SPACE[value]);

const SURFACE = { none: undefined, base: '$background', group: '$color2', raised: '$color3', accent: '$blue9', onAccent: 'rgba(255,255,255,0.2)' } as const;
const RADIUS = { none: 0, sm: '$2', md: '$4', lg: '$6', pill: '$10' } as const;
const ALIGN = { start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch' } as const;
const JUSTIFY = { start: 'flex-start', center: 'center', end: 'flex-end', between: 'space-between' } as const;

function box(props: LayoutProps): XStackProps {
  return {
    gap: space(props.gap),
    p: space(props.padding),
    px: space(props.paddingX),
    py: space(props.paddingY),
    pt: space(props.paddingTop),
    pb: space(props.paddingBottom),
    mx: space(props.marginX),
    mt: space(props.marginTop),
    mb: space(props.marginBottom),
    items: props.align && ALIGN[props.align],
    justify: props.justify && JUSTIFY[props.justify],
    flex: props.grow ? 1 : undefined,
    bg: props.surface && SURFACE[props.surface],
    rounded: props.radius && RADIUS[props.radius],
    ...(props.bordered ? { borderWidth: 1, borderColor: '$borderColor' } : {}),
    testID: props.testID,
  };
}

export function Row(props: LayoutProps) {
  return <XStack {...box(props)}>{props.children}</XStack>;
}

export function Stack(props: LayoutProps) {
  return <YStack {...box(props)}>{props.children}</YStack>;
}

const VARIANT: Record<NonNullable<TextProps['variant']>, DesignTextProps['variant']> = {
  body: 'default',
  title: 'h1',
  heading: 'h3',
  large: 'large',
  small: 'small',
  muted: 'muted',
  strong: 'strong',
};

export function Text(props: TextProps) {
  return (
    <DesignText
      variant={VARIANT[props.variant ?? 'body']}
      {...(props.tone === 'onAccent' ? { color: 'white' } : {})}
      opacity={props.dimmed ? 0.85 : undefined}
      flex={props.grow ? 1 : undefined}
      numberOfLines={props.numberOfLines}
      mt={space(props.marginTop)}
      mb={space(props.marginBottom)}
      testID={props.testID}
    >
      {props.children}
    </DesignText>
  );
}
