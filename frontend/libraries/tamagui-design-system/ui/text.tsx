import { SizableText, type SizableTextProps } from '../primitives';

export type TextVariant =
  | 'default'
  | 'h1'
  | 'h2'
  | 'h3'
  | 'h4'
  | 'large'
  | 'lead'
  | 'small'
  | 'muted'
  | 'strong'
  | 'code';

export type TextProps = SizableTextProps & { variant?: TextVariant };

/**
 * Text with the same `variant` vocabulary app #1's Text exposes, so a ported
 * screen reads the same even though the implementation is unrelated.
 *
 * Written as a plain component mapping variant -> props rather than
 * `styled(..., { variants })`. Tamagui's styled() variant types did not
 * survive the package boundary -- consumers saw the base SizableText props
 * and `variant` was rejected. An explicit lookup is less idiomatic and
 * actually works.
 *
 * App #1's variants came from shadcn's *website* -- `blockquote`, `lead`, `p`
 * with margins, `scroll-m-20`. That is documentation typography, not app
 * typography, so only the set the leetcode feature uses is reproduced.
 */
const VARIANT_PROPS: Record<TextVariant, SizableTextProps> = {
  default: { size: '$5' },
  h1: { size: '$10', fontWeight: '700' },
  h2: { size: '$9', fontWeight: '700' },
  h3: { size: '$8', fontWeight: '600' },
  h4: { size: '$7', fontWeight: '600' },
  large: { size: '$6', fontWeight: '600' },
  lead: { size: '$6', color: '$color11' },
  small: { size: '$3', fontWeight: '500' },
  muted: { size: '$3', color: '$color11' },
  strong: { fontWeight: '600' },
  // Tamagui types fontFamily as a config token, and defaultConfig ships no
  // mono family -- so the monospace face goes through `style`, which is not
  // token-checked, rather than inventing a token the config does not define.
  code: { size: '$3', style: { fontFamily: 'Menlo, Courier, monospace' } },
};

export function Text({ variant = 'default', ...rest }: TextProps) {
  return <SizableText color="$color" {...VARIANT_PROPS[variant]} {...rest} />;
}
