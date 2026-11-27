import { homePalettes, useOrcaScheme } from './appearance';

const chatPalettes = {
  light: {
    ...homePalettes.light,
    translucentFill: 'rgba(120, 120, 128, 0.16)',
    chatForegroundStrong: '#1e1e1e',
    syntax: {
      comment: '#59636e',
      keyword: '#cf222e',
      string: '#0a3069',
      number: '#0550ae',
      type: '#953800',
      function: '#8250df',
      variable: '#953800',
      meta: '#0550ae',
      tag: '#116329',
    },
    statusRed: '#e40014',
    sheetSurface: '#f5f5f5',
    userBorder: 'rgba(10, 10, 10, 0.07)',
    composerSurface: '#ffffff',
    composerBorder: 'rgba(10, 10, 10, 0.11)',
    textMuted: '#696969',
  },
  dark: {
    ...homePalettes.dark,
    translucentFill: 'rgba(120, 120, 128, 0.36)',
    chatForegroundStrong: '#e3e3e3',
    syntax: {
      comment: '#8b949e',
      keyword: '#ff7b72',
      string: '#a5d6ff',
      number: '#79c0ff',
      type: '#ffa657',
      function: '#d2a8ff',
      variable: '#ffa657',
      meta: '#79c0ff',
      tag: '#7ee787',
    },
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
