import { Redirect, useLocalSearchParams } from 'expo-router';

export default function ProblemSubmissionRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <Redirect href={{ pathname: '/leetcode/[id]/problem', params: { id } }} />;
}
