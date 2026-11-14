import { useLocalSearchParams } from 'expo-router';
import { useEffect } from 'react';

import { ProblemDescriptionSheet } from './ProblemDescriptionSheet';
import { ProblemCatalog } from '../domain/problemCatalog';
import { dismissOverlayRoute } from './dismissOverlayRoute';

export function ProblemDescriptionOverlay() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const problem = id ? ProblemCatalog.getById(id) : undefined;

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
