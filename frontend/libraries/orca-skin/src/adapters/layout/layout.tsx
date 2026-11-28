import type { LayoutProps, Space, TextProps } from '@lib/skin';
import { Text as NativeText, View, type TextStyle, type ViewStyle } from 'react-native';

import { homePalettes, homeRadii, homeSpacing, useOrcaScheme } from '../appearance';

const SPACE = { none: 0, xs: homeSpacing.xs, sm: homeSpacing.sm, md: homeSpacing.md, lg: homeSpacing.lg, xl: homeSpacing.xl } as const;
const space = (value: Space | undefined) => (value === undefined || typeof value === 'number' ? value : SPACE[value]);

const RADIUS = { none: 0, sm: homeRadii.mark, md: homeRadii.control, lg: homeRadii.surface, pill: homeRadii.pill } as const;
const ALIGN = { start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch' } as const;
const JUSTIFY = { start: 'flex-start', center: 'center', end: 'flex-end', between: 'space-between' } as const;

function useBox(props: LayoutProps, direction: 'row' | 'column'): ViewStyle {
  const colors = homePalettes[useOrcaScheme()];
  const surfaces = { none: undefined, base: colors.bgBase, group: colors.bgGroup, raised: colors.bgRaised, accent: colors.accentBlue, onAccent: 'rgba(255,255,255,0.2)' };
  return {
    flexDirection: direction,
    gap: space(props.gap),
    padding: space(props.padding),
    paddingHorizontal: space(props.paddingX),
    paddingVertical: space(props.paddingY),
    paddingTop: space(props.paddingTop),
    paddingBottom: space(props.paddingBottom),
    marginHorizontal: space(props.marginX),
    marginTop: space(props.marginTop),
    marginBottom: space(props.marginBottom),
    alignItems: props.align && ALIGN[props.align],
    justifyContent: props.justify && JUSTIFY[props.justify],
    flex: props.grow ? 1 : undefined,
    backgroundColor: props.surface && surfaces[props.surface],
    borderRadius: props.radius && RADIUS[props.radius],
    ...(props.bordered ? { borderWidth: 1, borderColor: colors.borderSubtle } : {}),
  };
}

export function Row(props: LayoutProps) {
  return <View testID={props.testID} style={useBox(props, 'row')}>{props.children}</View>;
}

export function Stack(props: LayoutProps) {
  return <View testID={props.testID} style={useBox(props, 'column')}>{props.children}</View>;
}

const VARIANT: Record<NonNullable<TextProps['variant']>, TextStyle> = {
  body: { fontSize: 15, lineHeight: 20 },
  title: { fontSize: 28, lineHeight: 34, fontWeight: '700' },
  heading: { fontSize: 17, lineHeight: 22, fontWeight: '600' },
  large: { fontSize: 17, lineHeight: 22, fontWeight: '600' },
  small: { fontSize: 12, lineHeight: 16, fontWeight: '500' },
  muted: { fontSize: 13, lineHeight: 18 },
  strong: { fontSize: 15, lineHeight: 20, fontWeight: '600' },
};

export function Text(props: TextProps) {
  const colors = homePalettes[useOrcaScheme()];
  const variant = props.variant ?? 'body';
  const color = props.tone === 'onAccent' ? colors.onAccent : variant === 'muted' ? colors.textSecondary : colors.textPrimary;
  return (
    <NativeText
      testID={props.testID}
      numberOfLines={props.numberOfLines}
      style={[VARIANT[variant], { color, opacity: props.dimmed ? 0.85 : undefined, flex: props.grow ? 1 : undefined, marginTop: space(props.marginTop), marginBottom: space(props.marginBottom) }]}
    >
      {props.children}
    </NativeText>
  );
}
