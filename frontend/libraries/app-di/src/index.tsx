import {
  createContext,
  useContext,
  useMemo,
  type JSX,
  type ReactNode,
} from 'react';

type Overrides = Record<string, object>;

const OverridesContext = createContext<Overrides | null>(null);

const EMPTY = {};

export function DependencyOverridesProvider(props: {
  overrides: Overrides;
  children: ReactNode;
}): JSX.Element {
  const parentOverrides = useContext(OverridesContext);
  const merged = useMemo(
    () => ({ ...parentOverrides, ...props.overrides }),
    [parentOverrides, props.overrides]
  );
  return (
    <OverridesContext.Provider value={merged}>
      {props.children}
    </OverridesContext.Provider>
  );
}

export function useDependencyOverrides<T extends object>(
  key: string
): Partial<T> {
  const overrides = useContext(OverridesContext);
  return overrides?.[key] ?? EMPTY;
}
