import { useLocalSearchParams } from 'expo-router';

import { devLang, devSeed } from '../ui/devSeed';
import { ProblemWorkspace } from '../ui/ProblemWorkspace';

export default function ProblemEditorRoute() {
  const { id, seed, seed64, lang } = useLocalSearchParams<{ id: string; seed?: string; seed64?: string; lang?: string }>();
  // a link to the open problem with another seed starts that screen afresh: expo-router updates params, it does not remount
  return <ProblemWorkspace key={`${id}:${seed64 ?? seed ?? ''}:${lang ?? ''}`} id={id} seed={devSeed({ seed, seed64 })} lang={devLang(lang)} />;
}