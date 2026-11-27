import type { ReactNode } from 'react';

/** A step of the skin's spacing scale, or points when a screen computes the distance (a safe-area inset). */
export type Space = 'none' | 'xs' | 'sm' | 'md' | 'lg' | 'xl' | number;

/** What a box is painted with; the skin owns the colors. */
export type Surface = 'none' | 'base' | 'group' | 'raised' | 'accent' | 'onAccent';

export interface LayoutProps {
  children?: ReactNode;
  gap?: Space;
  padding?: Space;
  paddingX?: Space;
  paddingY?: Space;
  paddingTop?: Space;
  paddingBottom?: Space;
  marginX?: Space;
  marginTop?: Space;
  marginBottom?: Space;
  align?: 'start' | 'center' | 'end' | 'stretch';
  justify?: 'start' | 'center' | 'end' | 'between';
  /** Takes the room its parent has left. */
  grow?: boolean;
  surface?: Surface;
  radius?: 'none' | 'sm' | 'md' | 'lg' | 'pill';
  /** A hairline around the box, in the skin's border color. */
  bordered?: boolean;
  testID?: string;
}

export interface TextProps {
  children?: ReactNode;
  variant?: 'body' | 'title' | 'heading' | 'large' | 'small' | 'muted' | 'strong';
  tone?: 'default' | 'onAccent';
  /** Less than fully opaque, for a secondary line on an accent surface. */
  dimmed?: boolean;
  grow?: boolean;
  numberOfLines?: number;
  marginTop?: Space;
  marginBottom?: Space;
  testID?: string;
}
