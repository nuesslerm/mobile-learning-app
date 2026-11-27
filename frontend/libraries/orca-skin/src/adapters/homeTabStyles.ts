import { StyleSheet } from 'react-native';
import {
  homePalettes,
  homeRadii,
  homeSpacing,
  useOrcaScheme,
} from './appearance';

function createStyles(
  colors: typeof homePalettes.light | typeof homePalettes.dark
) {
  return StyleSheet.create({
    header: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      minHeight: 44,
      paddingLeft: homeSpacing.lg + homeSpacing.xs,
      paddingRight: homeSpacing.sm,
    },
    headerTitle: {
      flexShrink: 1,
      fontSize: 22,
      fontWeight: '700',
      letterSpacing: -0.4,
      color: colors.textPrimary,
    },
    headerActions: { flexDirection: 'row', alignItems: 'center', gap: 2 },
    headerAction: {
      width: 44,
      height: 44,
      borderRadius: homeRadii.control,
      alignItems: 'center',
      justifyContent: 'center',
    },
    headerActionActive: { backgroundColor: colors.bgRaised },
    disabled: { opacity: 0.4 },
    empty: {
      alignItems: 'center',
      paddingHorizontal: homeSpacing.xl,
      paddingVertical: 28,
      marginTop: 32,
    },
    emptyTitle: {
      fontWeight: '600',
      color: colors.textPrimary,
    },
    emptyBody: {
      lineHeight: 20,
      color: colors.textSecondary,
      textAlign: 'center',
    },
    search: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: homeSpacing.sm,
      height: 40,
      marginHorizontal: homeSpacing.lg,
      marginBottom: homeSpacing.sm,
      paddingHorizontal: homeSpacing.md,
      borderRadius: homeRadii.control,
      backgroundColor: colors.bgRaised,
    },
    searchInput: { flex: 1, fontSize: 15, color: colors.textPrimary },
    searchClear: { padding: 4 },
    sheet: {
      paddingVertical: homeSpacing.sm,
      paddingBottom: homeSpacing.xl,
      borderTopLeftRadius: homeRadii.surface,
      borderTopRightRadius: homeRadii.surface,
      backgroundColor: colors.bgPanel,
    },
    sheetGroup: { paddingHorizontal: homeSpacing.sm },
    sheetRow: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      minHeight: 48,
      paddingHorizontal: homeSpacing.md,
    },
    sheetRowLabel: { flexShrink: 1, fontSize: 16, color: colors.textPrimary },
    settingsGroup: {
      borderRadius: homeRadii.surface,
      overflow: 'hidden',
    },
    settingsSeparator: {
      height: StyleSheet.hairlineWidth,
      backgroundColor: colors.borderSubtle,
    },
    settingsRow: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: homeSpacing.md,
      minHeight: 48,
      paddingHorizontal: homeSpacing.lg,
      paddingVertical: homeSpacing.sm,
    },
    settingsLeading: {
      width: 24,
      alignItems: 'center',
      justifyContent: 'center',
    },
    settingsLabel: { flex: 1, fontSize: 15, color: colors.textPrimary },
    settingsValue: {
      fontSize: 15,
      color: colors.textSecondary,
      flexShrink: 1,
      maxWidth: 180,
    },
    settingsAccessory: {
      fontSize: 15,
      fontWeight: '600',
      color: colors.textPrimary,
    },
    settingsScreen: { flex: 1 },
    settingsNavBar: {
      alignItems: 'center',
      justifyContent: 'center',
      height: 52 + StyleSheet.hairlineWidth,
      borderBottomWidth: StyleSheet.hairlineWidth,
      borderBottomColor: colors.borderSubtle,
    },
    settingsBack: {
      position: 'absolute',
      left: homeSpacing.md,
      width: 36,
      height: 36,
      borderRadius: homeRadii.pill,
      backgroundColor: colors.bgRaised,
      alignItems: 'center',
      justifyContent: 'center',
    },
    settingsNavTitle: {
      maxWidth: '62%',
      fontSize: 16,
      fontWeight: '600',
      color: colors.textPrimary,
    },
    settingsContent: {
      paddingHorizontal: homeSpacing.lg,
      paddingTop: homeSpacing.sm,
    },
    settingsNote: {
      fontSize: 13,
      lineHeight: 18,
      paddingHorizontal: homeSpacing.sm,
      paddingTop: homeSpacing.sm,
    },
  });
}

const byScheme = {
  light: createStyles(homePalettes.light),
  dark: createStyles(homePalettes.dark),
};

export function useHomeTabStyles() {
  return byScheme[useOrcaScheme()];
}
