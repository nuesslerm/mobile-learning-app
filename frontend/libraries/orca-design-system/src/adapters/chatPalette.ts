import { homePalettes, useOrcaScheme } from './appearance';

const chatPalettes = {
  light: {
    ...homePalettes.light,
    userBorder: 'rgba(10, 10, 10, 0.07)',
    composerSurface: '#ffffff',
    composerBorder: 'rgba(10, 10, 10, 0.11)',
    textMuted: '#696969',
  },
  dark: {
    ...homePalettes.dark,
    userBorder: 'rgba(250, 250, 250, 0.06)',
    composerSurface: '#1f1f1f',
    composerBorder: 'rgba(250, 250, 250, 0.09)',
    textMuted: '#898989',
  },
};

export function useChatPalette() {
  return chatPalettes[useOrcaScheme()];
}
