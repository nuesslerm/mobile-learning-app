import { Button, type ButtonProps } from '@expo/ui';
import { buttonStyle } from '@expo/ui/swift-ui/modifiers';
import { isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';

export function ExpoButton({ variant = 'filled', ...props }: ButtonProps) {
  const glass = isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
  return <Button {...props} variant={variant} modifiers={glass
    ? [buttonStyle(variant === 'filled' ? 'glassProminent' : 'glass'), ...(props.modifiers ?? [])]
    : props.modifiers} />;
}
