import { useLocalSearchParams } from 'expo-router';
import { useEffect } from 'react';

import { ProblemDescriptionSheet } from '../../ui/ProblemDescriptionSheet';
import { ProblemCatalog } from '../../data/problemCatalog';
import { dismissOverlayRoute } from '../../ui/dismissOverlayRoute';

export default function LeetCodeProblemDescriptionRoute() {
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