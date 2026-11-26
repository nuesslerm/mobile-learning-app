import { homePalettes, useOrcaScheme } from './appearance';

const chatPalettes = {
  light: {
    ...homePalettes.light,
    statusRed: '#e40014',
    sheetSurface: '#f5f5f5',
    userBorder: 'rgba(10, 10, 10, 0.07)',
    composerSurface: '#ffffff',
    composerBorder: 'rgba(10, 10, 10, 0.11)',
    textMuted: '#696969',
  },
  dark: {
    ...homePalettes.dark,
    statusRed: '#ff6568',
    sheetSurface: '#171717',
    userBorder: 'rgba(250, 250, 250, 0.06)',
    composerSurface: '#1f1f1f',
    composerBorder: 'rgba(250, 250, 250, 0.09)',
    textMuted: '#898989',
  },
};

export function useChatPalette() {
  return chatPalettes[useOrcaScheme()];
}
