import { describe, expect, it } from 'vitest';

import { fonts } from '../fonts';
import { tamaguiConfig } from '../tamagui.config';

const SPACING_KEYS = [
  '0', '0.5', '1', '1.5', '2', '2.5', '3', '3.5', '4', '4.5', '5', '5.5',
  '6', '6.5', '7', '7.5', '8', '8.5', '9', '9.5', '10', '10.5', '11', '11.5',
  '12', '12.5', '13', '13.5', '14', '14.5', '15', '15.5', '16', '16.5', '17',
  '17.5', '18', '18.5', '19', '19.5', '20', '24', '28', '32', '36', '40', '44',
  '48', '52', '56', '60', '64', '72', '80', '96', 'bottom-safe', 'px',
] as const;

const RADIUS_KEYS = [
  'none', 'sm', 'md', 'lg', 'pill', 'xl', 'nav', 'tile', 'card-lg',
] as const;

const Z_LAYER_KEYS = [
  'base', 'header', 'docked', 'sticky', 'nav', 'sheet', 'overlay', 'debug-surface',
] as const;

describe('given the design-system spacing source', () => {
  it('when the config is created, then space carries every transcribed key', () => {
    for (const key of SPACING_KEYS) {
      expect(tamaguiConfig.tokens.space).toHaveProperty(key);
    }
  });
});

describe('given the design-system radius source', () => {
  it('when the config is created, then radius carries every transcribed key', () => {
    for (const key of RADIUS_KEYS) {
      expect(tamaguiConfig.tokens.radius).toHaveProperty(key);
    }
  });
});

describe('given the design-system z-layer source', () => {
  it('when the config is created, then zIndex carries every transcribed key', () => {
    for (const key of Z_LAYER_KEYS) {
      expect(tamaguiConfig.tokens.zIndex).toHaveProperty(key);
    }
  });
});

describe('given the transcribed preset themes', () => {
  it('when the config is created, then light and dark themes exist', () => {
    expect(tamaguiConfig.themes).toHaveProperty('light');
    expect(tamaguiConfig.themes).toHaveProperty('dark');
  });
});

describe('given the transcribed fonts', () => {
  it('when the config is created, then body, serif and mono fonts exist', () => {
    expect(tamaguiConfig.fonts).toHaveProperty('body');
    expect(tamaguiConfig.fonts).toHaveProperty('serif');
    expect(tamaguiConfig.fonts).toHaveProperty('mono');
  });

  it('when fonts are created, then body font family uses the imported sans stack', () => {
    expect(tamaguiConfig.fonts.body?.family).toBe(
      'system-ui, -apple-system, Roboto, sans-serif'
    );
  });

  it('when fonts module is imported, then body font exists', () => {
    expect(fonts).toHaveProperty('body');
  });
});
