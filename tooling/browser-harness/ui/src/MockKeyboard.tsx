import { MOCK_KEYBOARD_HEIGHT } from '@cli/harness-contracts';

import { useState } from 'react';

// ---------------------------------------------------------------------------
// Key model
//
// A key is either a printable string (typed verbatim into the focused element)
// or a "special" with an action. Printable keys are sent to the daemon as
// {k:'text'} → page.keyboard.insertText, so uppercase letters, symbols, and
// unicode land reliably regardless of layout. Specials that must trigger
// editor behavior (delete, newline) are sent as {k:'press'} → page.keyboard
// .press with a real Playwright key name.
//
// This covers real typing "beyond simple keys": Shift for uppercase, a numbers
// layer, and a symbols layer with the punctuation code needs (brackets, braces,
// backtick, quotes, operators). Exotic keys (Tab, arrows, Escape, modifier
// combos) are intentionally left to `wf browser press-key`,
// which routes to the same focused element.
// ---------------------------------------------------------------------------

type SpecialAction =
  | 'shift'
  | 'bksp'
  | 'enter'
  | 'space'
  | 'layer-abc'
  | 'layer-num'
  | 'layer-sym';

type KeyDef = string | { label: string; action: SpecialAction; flex?: number };

type Layer = 'abc' | 'num' | 'sym';

const BOTTOM_ROW = (leftLabel: string, leftAction: SpecialAction): KeyDef[] => [
  { label: leftLabel, action: leftAction, flex: 2 },
  { label: 'space', action: 'space', flex: 5 },
  { label: 'return', action: 'enter', flex: 2 },
];

const LAYERS: Record<Layer, KeyDef[][]> = {
  abc: [
    ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'],
    ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'],
    [
      { label: '⇧', action: 'shift', flex: 1.5 },
      'z',
      'x',
      'c',
      'v',
      'b',
      'n',
      'm',
      { label: '⌫', action: 'bksp', flex: 1.5 },
    ],
    BOTTOM_ROW('123', 'layer-num'),
  ],
  num: [
    ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'],
    ['-', '/', ':', ';', '(', ')', '$', '&', '@', '"'],
    [
      { label: '#+=', action: 'layer-sym', flex: 1.5 },
      '.',
      ',',
      '?',
      '!',
      "'",
      { label: '⌫', action: 'bksp', flex: 1.5 },
    ],
    BOTTOM_ROW('ABC', 'layer-abc'),
  ],
  sym: [
    ['[', ']', '{', '}', '#', '%', '^', '*', '+', '='],
    ['_', '\\', '|', '~', '<', '>', '`', '"', "'"],
    [
      { label: '123', action: 'layer-num', flex: 1.5 },
      '.',
      ',',
      '?',
      '!',
      '§',
      { label: '⌫', action: 'bksp', flex: 1.5 },
    ],
    BOTTOM_ROW('ABC', 'layer-abc'),
  ],
};

// The keyboard's on-screen position, derived from the app iframe's rect (the
// device's actual screen).
//
// `top` is computed by the caller because it differs per platform: on Android
// the iframe has already been shrunk, so the keyboard fills the strip it
// vacated (`rect.bottom`); on iOS the iframe keeps its full height and the
// keyboard is drawn OVER its lower band (`rect.bottom - KEYBOARD_HEIGHT`).
export type ScreenAnchor = {
  left: number;
  width: number;
  top: number;
  // the screen's own bottom corner radius when the keyboard reaches the screen's bottom edge, so it is clipped like the
  // screen instead of drawing square corners over the rounded ones
  bottomRadius: number;
};

export const KEYBOARD_HEIGHT = MOCK_KEYBOARD_HEIGHT;

type Props = { visible: boolean; anchor: ScreenAnchor | null };

function sendText(value: string) {
  window.__shell.sendKey(JSON.stringify({ k: 'text', v: value }));
}
function sendPress(value: string) {
  window.__shell.sendKey(JSON.stringify({ k: 'press', v: value }));
}

export function MockKeyboard({ visible, anchor }: Props) {
  const [layer, setLayer] = useState<Layer>('abc');
  const [shift, setShift] = useState(false);

  if (!visible || !anchor) return null;

  const handleChar = (ch: string) => {
    const out = layer === 'abc' && shift ? ch.toUpperCase() : ch;
    sendText(out);
    if (shift) setShift(false); // one-shot shift, like iOS
  };

  const handleSpecial = (action: SpecialAction) => {
    switch (action) {
      case 'shift':
        setShift((s) => !s);
        break;
      case 'bksp':
        sendPress('Backspace');
        break;
      case 'enter':
        sendPress('Enter');
        break;
      case 'space':
        sendText(' ');
        break;
      case 'layer-abc':
        setLayer('abc');
        setShift(false);
        break;
      case 'layer-num':
        setLayer('num');
        break;
      case 'layer-sym':
        setLayer('sym');
        break;
    }
  };

  return (
    <div
      className="mock-keyboard"
      style={{
        position: 'fixed',
        left: anchor.left,
        width: anchor.width,
        top: anchor.top,
        height: KEYBOARD_HEIGHT,
        background: '#d1d5db',
        padding: 6,
        boxSizing: 'border-box',
        borderBottomLeftRadius: anchor.bottomRadius,
        borderBottomRightRadius: anchor.bottomRadius,
        overflow: 'hidden',
        boxShadow: '0 -8px 24px -12px rgba(0,0,0,0.25)',
        zIndex: 1000,
      }}
    >
      {LAYERS[layer].map((row, ri) => (
        <div
          key={ri}
          style={{
            display: 'flex',
            gap: 5,
            marginBottom: 6,
            justifyContent: 'center',
          }}
        >
          {row.map((def, ki) => {
            const isSpecial = typeof def !== 'string';
            const active = isSpecial && def.action === 'shift' && shift;
            const label = isSpecial
              ? def.label
              : layer === 'abc' && shift
                ? def.toUpperCase()
                : def;
            return (
              <button
                key={ki}
                // Stable, layer/shift-independent selector for E2E + the
                // daemon: the base char for printable keys, the action name
                // for specials.
                data-mcp-key={isSpecial ? def.action : def}
                // pointerdown (not click) so the press registers before the
                // focused element can blur.
                onPointerDown={(e) => {
                  e.preventDefault();
                  if (isSpecial) handleSpecial(def.action);
                  else handleChar(def);
                }}
                style={{
                  flex: isSpecial ? (def.flex ?? 1) : 1,
                  minWidth: 24,
                  height: 42,
                  borderRadius: 6,
                  border: '1px solid #b0b4b9',
                  background: active ? '#9aa0a6' : '#fff',
                  fontSize: 15,
                  cursor: 'pointer',
                  userSelect: 'none',
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
      ))}
    </div>
  );
}
