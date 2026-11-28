import { StyleSheet } from 'react-native';
import { homePalettes, homeRadii, homeSpacing, useOrcaScheme } from './appearance';

function createStyles(
  colors: typeof homePalettes.light | typeof homePalettes.dark
) {
  return StyleSheet.create({
    bar: {
      flexDirection: 'row',
      paddingTop: 6,
      paddingHorizontal: homeSpacing.xs,
      backgroundColor: colors.bgBase,
      borderTopWidth: StyleSheet.hairlineWidth,
      borderTopColor: colors.borderSubtle,
    },
    tab: {
      flex: 1,
      minHeight: 44,
      alignItems: 'center',
      gap: 3,
      paddingTop: 2,
    },
    icon: { width: 30, height: 26, alignItems: 'center', justifyContent: 'center' },
    label: { fontSize: 10.5, fontWeight: '500', letterSpacing: 0.1 },
    badge: {
      position: 'absolute',
      top: -4,
      left: 20,
      minWidth: 17,
      height: 17,
      paddingHorizontal: 4,
      borderRadius: homeRadii.pill,
      alignItems: 'center',
      justifyContent: 'center',
      backgroundColor: colors.statusAmber,
      borderWidth: 1.5,
      borderColor: colors.bgBase,
    },
    badgeText: { fontSize: 11, fontWeight: '700', color: '#000' },
  });
}

const byScheme = {
  light: createStyles(homePalettes.light),
  dark: createStyles(homePalettes.dark),
};

export function useHomeTabBarStyles() {
  return byScheme[useOrcaScheme()];
}
