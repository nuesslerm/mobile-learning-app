import { StyleSheet } from 'react-native';
import {
  homePalettes,
  homeSpacing as spacing,
  homeRadii as radii,
  useOrcaScheme,
} from './appearance';

const typography = { titleSize: 18, bodySize: 14 };
function createStyles(scheme: keyof typeof homePalettes) {
  const colors = {
    ...homePalettes[scheme],
    statusRed: scheme === 'light' ? '#e40014' : '#ff6568',
  };
  return StyleSheet.create({
    container: {
      flex: 1,
      backgroundColor: colors.bgBase,
      padding: spacing.lg,
    },
    backButton: {
      width: 36,
      height: 36,
      borderRadius: radii.control,
      borderCurve: 'continuous',
      alignItems: 'center',
      justifyContent: 'center',
      marginBottom: spacing.sm,
    },
    steps: {
      gap: spacing.sm,
      marginBottom: spacing.lg,
      marginLeft: 7,
    },
    step: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: spacing.sm,
    },
    stepBadge: {
      width: 22,
      height: 22,
      borderRadius: radii.pill,
      backgroundColor: colors.bgRaised,
      alignItems: 'center',
      justifyContent: 'center',
    },
    stepNumber: {
      fontSize: 12,
      fontWeight: '700',
      color: colors.textSecondary,
    },
    stepText: {
      fontSize: typography.bodySize,
      color: colors.textSecondary,
    },
    cameraWrap: {
      flex: 1,
      borderRadius: radii.surface,
      borderCurve: 'continuous',
      overflow: 'hidden',
    },
    // Reserve the camera slot during paste mode so the bottom action cannot jump upward.
    cameraPlaceholder: {
      flex: 1,
      backgroundColor: colors.bgPanel,
      borderRadius: radii.surface,
      borderCurve: 'continuous',
    },
    camera: {
      ...StyleSheet.absoluteFill,
    },
    reticle: {
      ...StyleSheet.absoluteFill,
      alignItems: 'center',
      justifyContent: 'center',
    },
    reticleFrame: {
      position: 'relative',
    },
    corner: {
      position: 'absolute',
      width: 28,
      height: 28,
      borderColor: 'rgba(255,255,255,0.7)',
    },
    cornerTL: {
      top: 0,
      left: 0,
      borderTopWidth: 2.5,
      borderLeftWidth: 2.5,
      borderTopLeftRadius: radii.mark,
    },
    cornerTR: {
      top: 0,
      right: 0,
      borderTopWidth: 2.5,
      borderRightWidth: 2.5,
      borderTopRightRadius: radii.mark,
    },
    cornerBL: {
      bottom: 0,
      left: 0,
      borderBottomWidth: 2.5,
      borderLeftWidth: 2.5,
      borderBottomLeftRadius: radii.mark,
    },
    cornerBR: {
      bottom: 0,
      right: 0,
      borderBottomWidth: 2.5,
      borderRightWidth: 2.5,
      borderBottomRightRadius: radii.mark,
    },
    centered: {
      flex: 1,
      alignItems: 'center',
      justifyContent: 'center',
    },
    title: {
      fontSize: typography.titleSize,
      fontWeight: '600',
      color: colors.textPrimary,
      marginBottom: spacing.sm,
    },
    subtitle: {
      maxWidth: 310,
      fontSize: typography.bodySize,
      color: colors.textSecondary,
      textAlign: 'center',
      marginBottom: spacing.xl,
      lineHeight: 20,
    },
    connectingText: {
      color: colors.textSecondary,
      fontSize: typography.bodySize,
      marginTop: spacing.lg,
    },
    logSlot: {
      width: '100%',
      marginTop: spacing.lg,
      paddingHorizontal: spacing.sm,
    },
    errorText: {
      color: colors.statusRed,
      fontSize: typography.bodySize,
      textAlign: 'center',
      marginBottom: spacing.xl,
      lineHeight: 20,
    },
    primaryButton: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'center',
      gap: spacing.xs,
      backgroundColor: colors.textPrimary,
      paddingHorizontal: spacing.xl,
      paddingVertical: spacing.sm + 2,
      borderRadius: radii.control,
      borderCurve: 'continuous',
    },
    primaryButtonText: {
      color: colors.bgBase,
      fontSize: typography.bodySize,
      fontWeight: '600',
    },
    pasteButton: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'center',
      gap: spacing.xs,
      marginTop: spacing.md,
      paddingVertical: spacing.sm,
      borderRadius: radii.control,
      borderCurve: 'continuous',
    },
    pasteButtonPressed: {
      opacity: 0.6,
    },
    pasteButtonText: {
      color: colors.textSecondary,
      fontSize: typography.bodySize,
      fontWeight: '500',
    },
    errorActions: {
      alignItems: 'center',
      gap: spacing.sm,
    },
    secondaryButton: {
      paddingHorizontal: spacing.xl,
      paddingVertical: spacing.sm,
      borderRadius: radii.control,
      borderCurve: 'continuous',
    },
    secondaryButtonText: {
      color: colors.textSecondary,
      fontSize: typography.bodySize,
      fontWeight: '500',
    },
  });
}
const styles = { light: createStyles('light'), dark: createStyles('dark') };
export function usePairingStyles() {
  return styles[useOrcaScheme()];
}
