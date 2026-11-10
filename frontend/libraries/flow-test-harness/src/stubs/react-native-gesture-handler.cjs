// react-native-gesture-handler ships untranspiled Flow syntax that the jsdom
// flow-test transform chokes on ("Unexpected token 'typeof'"), so screens that
// use a gesture reach the harness through this stub instead.
//
// The builders record their callbacks and GestureDetector drives them from DOM
// pointer events, so a test taps or drags the real element (fireEvent.pointer*)
// and the component's own handlers run, as under a finger. The lifecycle matches
// the flow-interactions contract:
//   Pan:  pointerdown -> onBegin; first move past minDistance (default 0, so the
//         first move activates) -> onStart, then onUpdate/onChange for that and
//         every later move; pointerup -> onEnd(e, true) if activated, then
//         onFinalize(e, activated).
//   Tap:  pointerdown -> onBegin; pointerup within 10px -> onStart, onEnd(e, true),
//         onFinalize(e, true); beyond slop -> onFinalize(e, false) only.
//   Race/Exclusive: at most one child activates; Pan wins once movement passes
//         slop, else Tap. Simultaneous: every child runs. enabled(false) never fires.
const React = require('react');

const TAP_SLOP = 10;
const HANDLERS = ['onBegin', 'onStart', 'onUpdate', 'onChange', 'onEnd', 'onFinalize', 'onTouchesDown', 'onTouchesMove', 'onTouchesUp'];
const CONFIG = [
  'runOnJS',
  'minDistance',
  'maxPointers',
  'minPointers',
  'numberOfTaps',
  'maxDuration',
  'minDuration',
  'enabled',
  'shouldCancelWhenOutside',
  'hitSlop',
  'activeOffsetX',
  'activeOffsetY',
  'failOffsetX',
  'failOffsetY',
  'simultaneousWithExternalGesture',
  'requireExternalGestureToFail',
  'withTestId',
];

function createGesture(type) {
  const gesture = { __type: type, __handlers: {}, __enabled: true, __minDistance: 0 };
  for (const name of CONFIG) gesture[name] = () => gesture;
  gesture.enabled = (on) => {
    gesture.__enabled = on !== false;
    return gesture;
  };
  gesture.minDistance = (n) => {
    gesture.__minDistance = n ?? 0;
    return gesture;
  };
  for (const name of HANDLERS) {
    gesture[name] = (fn) => {
      gesture.__handlers[name] = fn;
      return gesture;
    };
  }
  return gesture;
}

const composite = (type) => (...gestures) => ({ __type: type, __children: gestures });

function call(gesture, name, event, success) {
  const fn = gesture.__handlers[name];
  if (fn) fn(event, success);
}

function boxRect(host) {
  // The detector renders a display:contents div whose own rect is zero-sized in
  // jsdom; x/y are relative to the wrapped element, i.e. its first element child.
  const el = host && host.children && host.children.length > 0 ? host.children[0] : host;
  return el.getBoundingClientRect();
}

function pointFrom(e, host) {
  const rect = boxRect(host);
  const absoluteX = e.clientX ?? 0;
  const absoluteY = e.clientY ?? 0;
  return { x: absoluteX - rect.left, y: absoluteY - rect.top, absoluteX, absoluteY };
}

function eventFor(p, start, prev) {
  return {
    x: p.x,
    y: p.y,
    absoluteX: p.absoluteX,
    absoluteY: p.absoluteY,
    translationX: p.x - start.x,
    translationY: p.y - start.y,
    changeX: p.x - prev.x,
    changeY: p.y - prev.y,
    velocityX: 0,
    velocityY: 0,
    numberOfPointers: 1,
  };
}

function distance(a, b) {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

// Build the gesture tree for one pointer session. Disabled leaves are dropped.
function buildTree(gesture) {
  if (gesture.__children) {
    const children = gesture.__children.map(buildTree).filter(Boolean);
    if (children.length === 0) return null;
    return { type: gesture.__type, children, claimed: false, claimedChild: null };
  }
  if (gesture.__enabled === false) return null;
  return { gesture, active: false };
}

function walkBegin(node, event) {
  if (node.gesture) {
    call(node.gesture, 'onBegin', event);
    return;
  }
  for (const child of node.children) walkBegin(child, event);
}

// Returns the leaf that claimed (activated), if any.
function walkMove(node, p, event, start) {
  if (node.gesture) {
    const g = node.gesture;
    if (g.__type !== 'Pan') return null;
    if (node.active) {
      call(g, 'onUpdate', event);
      call(g, 'onChange', event);
      return node;
    }
    if (distance(p, start) >= g.__minDistance) {
      node.active = true;
      call(g, 'onStart', event);
      call(g, 'onUpdate', event);
      call(g, 'onChange', event);
      return node;
    }
    return null;
  }
  if (node.type === 'Simultaneous') {
    let claiming = null;
    for (const child of node.children) {
      const claim = walkMove(child, p, event, start);
      if (claim && !claiming) claiming = claim;
    }
    return claiming;
  }
  if (node.claimed) {
    return node.claimedChild ? walkMove(node.claimedChild, p, event, start) : null;
  }
  let claiming = null;
  for (const child of node.children) {
    if (claiming) break;
    const claim = walkMove(child, p, event, start);
    if (claim) {
      claiming = claim;
      node.claimed = true;
      node.claimedChild = child;
    }
  }
  return claiming;
}

function walkUp(node, p, event, start, blocked) {
  if (node.gesture) {
    const g = node.gesture;
    if (node.active) {
      call(g, 'onEnd', event, true);
      call(g, 'onFinalize', event, true);
      return;
    }
    if (g.__type === 'Tap' && !blocked && distance(p, start) <= TAP_SLOP) {
      call(g, 'onStart', event);
      call(g, 'onEnd', event, true);
      call(g, 'onFinalize', event, true);
      return;
    }
    call(g, 'onFinalize', event, false);
    return;
  }
  if (node.type === 'Simultaneous') {
    for (const child of node.children) walkUp(child, p, event, start, blocked);
    return;
  }
  for (let i = 0; i < node.children.length; i++) {
    const child = node.children[i];
    const childBlocked = blocked || (node.claimed && child !== node.claimedChild);
    walkUp(child, p, event, start, childBlocked);
  }
}

const GestureDetector = ({ gesture, children }) => {
  const session = React.useRef(null);

  const onPointerDown = (e) => {
    const p = pointFrom(e, e.currentTarget);
    const tree = buildTree(gesture);
    if (!tree) return;
    walkBegin(tree, eventFor(p, p, p));
    session.current = { start: p, prev: p, tree };
  };
  const onPointerMove = (e) => {
    const s = session.current;
    if (!s) return;
    const p = pointFrom(e, e.currentTarget);
    walkMove(s.tree, p, eventFor(p, s.start, s.prev), s.start);
    s.prev = p;
  };
  const onPointerUp = (e) => {
    const s = session.current;
    session.current = null;
    if (!s) return;
    const p = pointFrom(e, e.currentTarget);
    walkUp(s.tree, p, eventFor(p, s.start, s.prev), s.start, false);
  };

  return React.createElement(
    'div',
    { style: { display: 'contents' }, onPointerDown, onPointerMove, onPointerUp },
    children
  );
};

const GestureHandlerRootView = React.forwardRef(({ children, ...props }, ref) =>
  React.createElement('div', { ...props, ref }, children)
);
GestureHandlerRootView.displayName = 'GestureHandlerRootView';

module.exports = {
  __esModule: true,
  Gesture: {
    Pan: () => createGesture('Pan'),
    Tap: () => createGesture('Tap'),
    LongPress: () => createGesture('LongPress'),
    Pinch: () => createGesture('Pinch'),
    Rotation: () => createGesture('Rotation'),
    Fling: () => createGesture('Fling'),
    Native: () => createGesture('Native'),
    Manual: () => createGesture('Manual'),
    Simultaneous: composite('Simultaneous'),
    Race: composite('Race'),
    Exclusive: composite('Exclusive'),
  },
  GestureDetector,
  GestureHandlerRootView,
  Directions: { RIGHT: 1, LEFT: 2, UP: 4, DOWN: 8 },
  State: {
    UNDETERMINED: 0,
    FAILED: 1,
    BEGAN: 2,
    CANCELLED: 3,
    ACTIVE: 4,
    END: 5,
  },
};