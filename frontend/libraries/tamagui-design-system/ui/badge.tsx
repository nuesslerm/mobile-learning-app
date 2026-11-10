import { SizableText, type SizableTextProps } from '../primitives';

type BadgeVariant =
  | 'default'
  | 'primary'
  | 'secondary'
  | 'destructive'
  | 'success'
  | 'warning'
  | 'outline';

export type BadgeProps = SizableTextProps & { variant?: BadgeVariant };

const VARIANT_PROPS: Record<BadgeVariant, SizableTextProps> = {
  default: { bg: '$color4', color: '$color' },
  primary: { bg: '$blue9', color: '$color1' },
  secondary: { bg: '$color3', color: '$color11' },
  destructive: { bg: '$red9', color: '$color1' },
  success: { bg: '$green9', color: '$color1' },
  warning: { bg: '$yellow9', color: '$color1' },
  outline: {
    bg: 'transparent',
    borderWidth: 1,
    borderColor: '$borderColor',
    color: '$color',
  },
};

/**
 * Status pill. Capsule-shaped, which is what app #1's Badge actually ships
 * despite its own solution doc insisting on a rounded rect -- the doc has
 * been wrong about reality for a while.
 */
export function Badge({ variant = 'default', ...rest }: BadgeProps) {
  return (
    <SizableText
      px="$2.5"
      py="$1"
      rounded={9999}
      size="$2"
      fontWeight="600"
      overflow="hidden"
      whiteSpace="nowrap"
      shrink={0}
      {...VARIANT_PROPS[variant]}
      {...rest}
    />
  );
}
