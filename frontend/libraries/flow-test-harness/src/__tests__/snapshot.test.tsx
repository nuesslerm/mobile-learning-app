import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Text } from '@lib/tamagui-design-system/primitives';
import { View } from 'react-native';
import { afterEach, describe, expect, it } from 'vitest';

import { screen, screenshot } from '../index';
import { layoutBoxes, setLayout } from '../interactions/layout';
import { snapshotPage } from '../interactions/snapshot';
import { renderScreen } from '../render';

afterEach(() => {
  delete process.env.FLOW_GOLDENS;
});

describe('golden snapshot', () => {
  it('given no goldens flag, when a test takes a screenshot, then the frozen page is still written under its name', async () => {
    // Given a rendered screen and an output folder
    delete process.env.FLOW_GOLDENS;
    const outDir = mkdtempSync(path.join(tmpdir(), 'flow-shots-'));
    process.env.FLOW_GOLDENS_OUT = outDir;
    try {
      renderScreen(<Text testID="heading">hello</Text>);
      // When the test takes a screenshot
      const written = await screenshot('checkpoint');
      // Then the page is kept, named after the test file and the screenshot
      expect(written.endsWith(path.join('snapshot', 'checkpoint.html'))).toBe(
        true
      );
      expect(readFileSync(written, 'utf8')).toContain('hello');
    } finally {
      delete process.env.FLOW_GOLDENS_OUT;
    }
  });

  it('given FLOW_GOLDENS=1 and a laid-out screen, when snapshotPage runs, then the HTML contains the text, styles, viewport meta and the laid-out width', () => {
    process.env.FLOW_GOLDENS = '1';
    renderScreen(
      <>
        <Text testID="heading">Hello golden</Text>
        <View testID="canvas" />
      </>
    );
    setLayout(screen.getByTestId('canvas'), {
      x: 0,
      y: 0,
      width: 320,
      height: 240,
    });
    const outDir = mkdtempSync(path.join(tmpdir(), 'flow-goldens-'));

    const written = snapshotPage('golden-check', {
      suite: 'snapshot',
      outDir,
      layouts: layoutBoxes(),
    });

    expect(written).not.toBeNull();
    const html = readFileSync(written as string, 'utf8');
    expect(html).toContain('Hello golden');
    expect(html).toMatch(/<style[\s>]/);
    expect(html).toContain(
      '<meta name="viewport" content="width=device-width, initial-scale=1">'
    );
    expect(html).toMatch(/width:\s*320px/);
  });
});
