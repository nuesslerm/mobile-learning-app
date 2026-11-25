import { HeaderHeightContext } from 'expo-router/react-navigation';
import { useContext } from 'react';

export function useNativeStackHeaderHeight(): number | undefined {
  return useContext(HeaderHeightContext);
}
