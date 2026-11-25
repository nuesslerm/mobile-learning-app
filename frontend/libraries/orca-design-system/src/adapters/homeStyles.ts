import { StyleSheet } from 'react-native';
import { homePalettes, useOrcaScheme } from './appearance';

const STEP_NUMBER_SIZE = 26;

function createStyles(
  colors: typeof homePalettes.light | typeof homePalettes.dark
) {
  return StyleSheet.create({
    container: { flexGrow: 1, justifyContent: 'center', padding: 16 },
    hero: { alignItems: 'center', paddingHorizontal: 8, marginBottom: 34 },
    title: {
      fontSize: 28,
      fontWeight: '700',
      letterSpacing: -0.4,
      color: colors.textPrimary,
      textAlign: 'center',
      marginTop: 26,
      marginBottom: 10,
    },
    body: {
      fontSize: 17,
      lineHeight: 24,
      color: colors.textSecondary,
      textAlign: 'center',
    },
    steps: {
      backgroundColor: colors.bgPanel,
      borderRadius: 16,
      borderCurve: 'continuous',
      overflow: 'hidden',
    },
    separator: {
      height: StyleSheet.hairlineWidth,
      backgroundColor: colors.borderSubtle,
      marginLeft: 16 + STEP_NUMBER_SIZE + 14,
    },
    step: {
      flexDirection: 'row',
      alignItems: 'flex-start',
      gap: 14,
      padding: 16,
    },
    stepNumber: {
      width: STEP_NUMBER_SIZE,
      height: STEP_NUMBER_SIZE,
      borderRadius: 999,
      backgroundColor: colors.bgRaised,
      alignItems: 'center',
      justifyContent: 'center',
    },
    stepNumberText: {
      fontSize: 14,
      fontWeight: '600',
      color: colors.textPrimary,
      fontVariant: ['tabular-nums'],
    },
    stepText: { flex: 1, minWidth: 0 },
    stepTitle: { fontSize: 17, lineHeight: 22, color: colors.textPrimary },
    stepDescription: {
      fontSize: 15,
      lineHeight: 20,
      color: colors.textSecondary,
      marginTop: 2,
    },
    ctaHit: { alignSelf: 'center', marginTop: 22 },
    cta: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: 8,
      minHeight: 48,
      paddingHorizontal: 22,
      borderRadius: 12,
      borderCurve: 'continuous',
      backgroundColor: colors.accentBlue,
    },
    ctaLabel: { fontSize: 16, fontWeight: '600', color: colors.onAccent },
  });
}

const byScheme = {
  light: createStyles(homePalettes.light),
  dark: createStyles(homePalettes.dark),
};
export function useHomeStyles() {
  return byScheme[useOrcaScheme()];
}
