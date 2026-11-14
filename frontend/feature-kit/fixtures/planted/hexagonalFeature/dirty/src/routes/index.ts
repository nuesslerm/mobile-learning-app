import { Container } from '../di/Container';
import { Screen } from '../ui/Screen';
export const route = (): number => Container() + Screen();
