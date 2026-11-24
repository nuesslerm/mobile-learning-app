import { useLocalSearchParams } from 'expo-router';

import { SubmissionDetailScreen } from '../../../ui/library/SubmissionDetailScreen';

export default function ProblemSubmissionRoute() {
  const { id, submissionId } = useLocalSearchParams<{ id: string; submissionId: string }>();
  return <SubmissionDetailScreen key={`${id}:${submissionId}`} id={id} submissionId={submissionId} />;
}
