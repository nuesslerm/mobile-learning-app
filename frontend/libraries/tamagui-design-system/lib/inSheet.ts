import { createContext, useContext } from 'react';

const InSheetContext = createContext(false);

/** Wrapped around a `Sheet` body, so a field anywhere below it, however deep, knows it must lift the sheet above the keyboard. */
export const InSheetProvider = InSheetContext.Provider;

export function useInSheet(): boolean {
  return useContext(InSheetContext);
}
