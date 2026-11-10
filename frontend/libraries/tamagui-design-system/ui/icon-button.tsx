import type { LucideIcon } from 'lucide-react-native';

import { Button, type ButtonProps } from './button';

export type IconButtonProps = Omit<ButtonProps, 'icon' | 'children'> & {
  icon: LucideIcon;
  iconSize?: number;
};

/** Icon-only circular action -- factors out the repeated round-button shape. */
export function IconButton({
  icon,
  iconSize = 20,
  variant = 'ghost',
  ...rest
}: IconButtonProps) {
  return <Button variant={variant} icon={icon} circular size="$4" {...rest} />;
}
