import { useLocalSearchParams } from 'expo-router';
import { useEffect } from 'react';

import { ProblemDescriptionSheet } from './ProblemDescriptionSheet';
import { useLeetCodePorts } from '../state/ports';
import { useDismissOverlayRoute } from './dismissOverlayRoute';

export function ProblemDescriptionOverlay() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const dismiss = useDismissOverlayRoute(id);
  const { libraryContent } = useLeetCodePorts();
  const problem = id ? libraryContent.problem(id) : undefined;

  useEffect(() => {
    if (!problem) dismiss();
  }, [problem, dismiss]);

  if (!problem) return null;

  return (
    <ProblemDescriptionSheet
      problem={problem}
      onClose={dismiss}
    />
  );
}
