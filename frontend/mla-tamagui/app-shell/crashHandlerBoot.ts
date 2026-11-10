import { installCrashHandler } from './crashHandler';

// A side-effect module so `app/_layout.tsx` can install the handler right after the logger, before its other imports
// evaluate: a throw in one of them would otherwise reach React Native's red box and leave no app.crash line.
installCrashHandler();
