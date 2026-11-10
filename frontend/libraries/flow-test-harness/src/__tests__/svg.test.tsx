import { render } from '@testing-library/react';
import Svg, { Path } from 'react-native-svg';
import { describe, expect, it } from 'vitest';

import { screen } from '../index';

describe('react-native-svg', () => {
  it('given a Path with a d prop, when the svg renders, then the d attribute is readable in the DOM', () => {
    render(
      <Svg>
        <Path testID="the-path" d="M 0 0 L 10 10" />
      </Svg>
    );
    expect(screen.getByTestId('the-path').getAttribute('d')).toBe(
      'M 0 0 L 10 10'
    );
  });
});