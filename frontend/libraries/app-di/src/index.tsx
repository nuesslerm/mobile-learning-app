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

declare const keyType: unique symbol;

/** A typed name for one feature's overrides: the container and the flow test share it, so both see the same port type. */
export type OverrideKey<T extends object> = {
  readonly name: string;
  readonly [keyType]?: T;
};

export function overrideKey<T extends object>(name: string): OverrideKey<T> {
  return { name } as OverrideKey<T>;
}

export function useDependencyOverrides<T extends object>(
  key: OverrideKey<T>
): Partial<T>;
/** The string form stays until every feature has an `overrideKey`. */
export function useDependencyOverrides<T extends object>(
  key: string
): Partial<T>;
export function useDependencyOverrides<T extends object>(
  key: string | OverrideKey<T>
): Partial<T> {
  const overrides = useContext(OverridesContext);
  const name = typeof key === 'string' ? key : key.name;
  return overrides?.[name] ?? EMPTY;
}
