import { useLocalSearchParams } from 'expo-router';
import { useEffect } from 'react';

import { ProblemDescriptionSheet } from './ProblemDescriptionSheet';
import { useLeetCodePorts } from '../state/ports';
import { dismissOverlayRoute } from './dismissOverlayRoute';

export function ProblemDescriptionOverlay() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { libraryContent } = useLeetCodePorts();
  const problem = id ? libraryContent.problem(id) : undefined;

  useEffect(() => {
    if (!problem) dismissOverlayRoute(id);
  }, [problem, id]);

  if (!problem) return null;

  return (
    <ProblemDescriptionSheet
      problem={problem}
      onClose={() => {
        dismissOverlayRoute(id);
      }}
    />
  );
}
