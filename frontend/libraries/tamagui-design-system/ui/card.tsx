import { YStack, type YStackProps } from '../primitives';

type CardTone = 'default' | 'accent' | 'danger' | 'warning';

export type CardProps = YStackProps & { tone?: CardTone };

const CARD_TONE_PROPS: Record<CardTone, YStackProps> = {
  default: { bg: '$color2' },
  accent: { bg: '$color4', borderWidth: 1, borderColor: '$borderColor' },
  danger: { bg: '$red3' },
  warning: { bg: '$yellow3' },
};

export function Card({ tone = 'default', ...rest }: CardProps) {
  return (
    <YStack rounded="$4" p="$3" gap="$2" {...CARD_TONE_PROPS[tone]} {...rest} />
  );
}

export type CardHeaderProps = YStackProps;

export function CardHeader(props: CardHeaderProps) {
  return <YStack gap="$1.5" {...props} />;
}

export type CardContentProps = YStackProps;

export function CardContent(props: CardContentProps) {
  return <YStack gap="$2" {...props} />;
}
