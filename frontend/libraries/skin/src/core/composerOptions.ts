import { useCallback, useEffect, useRef, useState } from 'react';
import { Keyboard } from 'react-native';

import type { ComposerOptionsProps } from '../ports/ChatComponents';

type Choice = { slug: string; name: string };
export type ComposerOptionsPage = 'Model' | 'Permissions' | 'Effort';

/** What the composer's options do, whatever they look like: which page shows, the models as they load, and a choice as it is saved. */
export function useComposerOptions(props: ComposerOptionsProps) {
  const [page, setPage] = useState<ComposerOptionsPage>('Model');
  const [models, setModels] = useState<readonly Choice[]>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const operation = useRef(0);
  const { visible, loadModels } = props;
  const opensEffort = Boolean(props.effort);
  const load = useCallback(() => {
    setError('');
    setModels(undefined);
    const request = ++operation.current;
    void loadModels().then(
      (choices) => {
        if (request === operation.current) setModels(choices);
      },
      () => {
        if (request === operation.current) setError('Could not load models.');
      }
    );
  }, [loadModels]);
  useEffect(() => {
    if (!visible) return;
    Keyboard.dismiss();
    setPage(opensEffort ? 'Effort' : 'Model');
    setBusy(false);
    setError('');
    if (!opensEffort) load();
    return () => {
      operation.current += 1;
    };
  }, [visible, load, opensEffort]);
  function returnToModels() {
    if (page === 'Effort') load();
    setPage('Model');
  }
  function select(slug: string) {
    if (busy) return;
    setError('');
    setBusy(true);
    const request = ++operation.current;
    const save = page === 'Effort' ? props.effort?.select : page === 'Model' ? props.setModel : props.setPermission;
    if (!save) {
      setBusy(false);
      return;
    }
    void save(slug).then(
      () => {
        if (request !== operation.current) return;
        setBusy(false);
        returnToModels();
      },
      () => {
        if (request !== operation.current) return;
        setError('Could not save this choice.');
        setBusy(false);
      }
    );
  }
  const modelName = models?.find((item) => item.slug === props.selectedModel)?.name;
  return {
    page,
    setPage,
    error,
    busy,
    load,
    returnToModels,
    select,
    permission: props.permissions.find((item) => item.slug === props.permission),
    /** The name on the control that opens the options. */
    label: props.selectedModel && props.selectedModel !== 'default' ? (modelName ?? props.selectedModel) : 'Model',
    choices: page === 'Effort' ? props.effort?.choices : page === 'Permissions' ? props.permissions : models,
    selected: page === 'Effort' ? props.effort?.current : page === 'Permissions' ? props.permission : props.selectedModel,
  };
}
