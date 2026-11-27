import type { AgentTheme } from '@lib/agent-ui-contracts';

/** Builds the agent's colors from a Tamagui theme; `read` answers a theme name such as `color11`. */
export function agentThemeFrom(
  read: (name: string) => string | undefined,
  scheme: 'light' | 'dark'
): AgentTheme {
  const dark = scheme === 'dark';
  const pick = (name: string, fallback: string) => read(name) ?? fallback;
  const background = pick('background', dark ? '#000000' : '#ffffff');
  const ink = pick('color', dark ? '#ffffff' : '#000000');
  const raised = pick('color3', dark ? '#2c2c2e' : '#f2f2f7');
  const group = pick('color2', dark ? '#1c1c1e' : '#f9f9f9');
  const border = pick('borderColor', dark ? '#38383a' : '#d1d1d6');
  const secondary = pick('color11', dark ? '#aeaeb2' : '#636366');
  const muted = pick('color10', dark ? '#8e8e93' : '#8e8e93');
  const accent = pick('blue9', dark ? '#0a84ff' : '#007aff');
  const red = pick('red9', dark ? '#ff453a' : '#ff3b30');
  const green = pick('green9', dark ? '#30d158' : '#34c759');
  const orange = pick('orange9', dark ? '#ff9f0a' : '#ff9500');
  return {
    scheme,
    colors: {
      bgBase: background,
      bgPanel: group,
      bgGroup: group,
      bgRaised: raised,
      borderSubtle: border,
      textPrimary: ink,
      textSecondary: secondary,
      textMuted: muted,
      surfaceBright: pick('color12', ink),
      accentBlue: accent,
      onAccent: '#ffffff',
      statusGreen: green,
      statusAmber: pick('yellow9', orange),
      statusRed: red,
      translucentFill: pick('color4', raised),
      chatForegroundStrong: pick('color12', ink),
      sheetSurface: group,
      userBorder: border,
      composerSurface: group,
      composerBorder: border,
      syntax: {
        comment: muted,
        keyword: red,
        string: accent,
        number: pick('blue10', accent),
        type: orange,
        function: pick('purple9', accent),
        variable: orange,
        meta: pick('blue10', accent),
        tag: green,
      },
    },
    tabs: {
      bg: background,
      surface: raised,
      ink,
      muted: secondary,
      border,
      headerInk: ink,
      focus: accent,
      danger: red,
      recording: red,
      scrim: 'rgba(0,0,0,0.74)',
      terminal: { background: '#191b26', foreground: '#e1e4ed', cursor: '#c3cbdf' },
    },
  };
}
