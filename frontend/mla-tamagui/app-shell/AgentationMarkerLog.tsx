import { Logger } from '@lib/logging';
import { useAgentation } from '@lib/rn-agentation';
import * as React from 'react';

export function AgentationMarkerLog(): null {
  const { selectedId } = useAgentation();

  // eslint-disable-next-line mobile-learn/restrict-use-effect -- mount init: logs marker selection
  React.useEffect(() => {
    if (selectedId) {
      Logger.info('agentation.marker.tapped', { annotationId: selectedId });
    }
  }, [selectedId]);

  return null;
}
