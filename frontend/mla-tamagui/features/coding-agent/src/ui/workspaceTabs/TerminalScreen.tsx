'use dom';

import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import type { DOMProps } from 'expo/dom';

type Props = {
  output: string;
  snapshot: number;
  fontSize: number;
  cols: number;
  rows: number;
  dom?: DOMProps;
};

export default function TerminalScreen({
  output,
  snapshot,
  fontSize,
  cols,
  rows,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);
  const previous = useRef({ snapshot: -1, output: '' });
  useEffect(() => {
    if (!host.current) return;
    const screen = new Terminal({
      disableStdin: true,
      fontFamily: 'Menlo, monospace',
      cursorBlink: false,
      allowTransparency: false,
      scrollback: 5000,
      theme: {
        background: '#191b26',
        foreground: '#e1e4ed',
        cursor: '#c3cbdf',
      },
    });
    terminal.current = screen;
    screen.open(host.current);
    screen.textarea?.setAttribute('aria-hidden', 'true');
    return () => {
      screen.dispose();
      terminal.current = null;
      previous.current = { snapshot: -1, output: '' };
    };
  }, []);
  useEffect(() => {
    const screen = terminal.current;
    if (!screen) return;
    screen.options.fontSize = fontSize;
    screen.resize(cols, rows);
  }, [cols, rows, fontSize]);
  useEffect(() => {
    const screen = terminal.current;
    if (!screen) return;
    const last = previous.current;
    const append = last.snapshot === snapshot && output.startsWith(last.output);
    screen.write(append ? output.slice(last.output.length) : '\x1bc' + output);
    previous.current = { output, snapshot };
  }, [output, snapshot]);
  return (
    <div
      aria-label="Terminal output"
      ref={host}
      style={{
        position: 'absolute',
        inset: 0,
        overflow: 'hidden',
        background: '#191b26',
      }}
    />
  );
}
