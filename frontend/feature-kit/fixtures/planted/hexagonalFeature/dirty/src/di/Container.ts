import { adapter } from '../data/adapter';
import { useThing } from '../state/useThing';
export const Container = (): number => adapter() + useThing();
