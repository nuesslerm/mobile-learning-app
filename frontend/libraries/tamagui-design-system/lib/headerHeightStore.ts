import { create } from 'zustand';

type HeaderHeightState = {
  height: number;
  setHeight: (height: number) => void;
};

/**
 * The screen header's MEASURED height, for overlays that must sit clear of it.
 *
 * A floating host mounted at the app root cannot see the header: the toast
 * layer renders above every screen, so at sonner-native's default `offset: 0`
 * a toast lands on the title.
 *
 * Computing the height from the header's own padding constants was tried first
 * and was wrong by 12px -- it gave 59 against a real 71, because the header's
 * content row is whatever its title and subtitle need, not a fixed control
 * height. Two lines of title make it taller again. So the header reports what
 * it actually laid out, and nothing has to predict it.
 */
export const useHeaderHeightStore = create<HeaderHeightState>()((set) => ({
  height: 0,
  setHeight: (height) =>
    set((s) =>
      Math.round(height) === s.height ? s : { height: Math.round(height) }
    ),
}));
