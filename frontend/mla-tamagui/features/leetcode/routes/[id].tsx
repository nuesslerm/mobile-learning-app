import { useLocalSearchParams } from 'expo-router';
import { useSyncExternalStore } from 'react';

import { devSeed } from '../ui/devSeed';
import { ProblemWorkspace } from '../ui/ProblemWorkspace';

const subscribeNever = () => () => undefined;

export default function ProblemEditorRoute() {
  const params = useLocalSearchParams<{ id: string; seed?: string; seed64?: string; lang?: string }>();
  // The export holds one template page for every id: hydrate as that page, then open the problem the address names.
  const hydrated = useSyncExternalStore(subscribeNever, () => true, () => false);
  const { id, seed, seed64, lang } = hydrated ? params : { id: '[id]' };
  // a link to the open problem with another seed starts that screen afresh: expo-router updates params, it does not remount
  return <ProblemWorkspace key={`${id}:${seed64 ?? seed ?? ''}:${lang ?? ''}`} id={id} seed={devSeed({ seed, seed64 })} lang={lang === 'javascript' ? 'javascript' : undefined} />;
}
