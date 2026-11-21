import builtins
import contextlib
import copy
import io
import json
import math
import sys
import time
import traceback
from collections import deque
from typing import Any, Deque, Dict, List, Optional, Set, Tuple

USER_FILE = '<string>'
OUTPUT_CAP = 256 * 1024
MAX_NODES = 1_000_000
OUTPUT_LIMIT = 4 * 1024 * 1024
MAX_RECURSION = 4000
APPROX_TOLERANCE = 1e-5


class ListNode:
    def __init__(self, val=0, next=None):
        self.val = val; self.next = next
    @classmethod
    def from_list(cls, lst):
        if not lst: return None
        head = cls(lst[0]); cur = head
        for v in lst[1:]:
            cur.next = cls(v); cur = cur.next
        return head
    def to_list(self):
        out = []; cur = self
        while cur: out.append(cur.val); cur = cur.next
        return out


class TreeNode:
    def __init__(self, val=0, left=None, right=None):
        self.val = val; self.left = left; self.right = right


class Node:
    def __init__(self, val=0, neighbors=None):
        self.val = val
        self.neighbors = neighbors if neighbors is not None else []


# BaseException so a solution's own `except Exception` cannot swallow the deadline
class TimeoutError(BaseException):
    pass


class _Capped(io.StringIO):
    size = 0

    def write(self, text):
        taken = len(text)
        if self.size >= OUTPUT_CAP:
            return taken
        self.size += taken
        if self.size > OUTPUT_CAP:
            text = text[: taken - (self.size - OUTPUT_CAP)] + '\n...[output truncated]'
        super().write(text)
        return taken


class _Guard:
    def __init__(self):
        mon = getattr(sys, 'monitoring', None)
        if mon is None:
            raise RuntimeError('the judge needs Python 3.12 or newer (sys.monitoring)')
        self.mon = mon
        self.disable = mon.DISABLE
        self.tool = None
        for tool in (3, 4, 2):
            try:
                mon.use_tool_id(tool, 'leetcode-judge')
            except ValueError:
                continue
            self.tool = tool
            break
        if self.tool is None:
            raise RuntimeError('no free sys.monitoring tool id')
        ev = mon.events
        self.events = ev.JUMP | ev.PY_START | ev.PY_RESUME
        for event in (ev.JUMP, ev.PY_START, ev.PY_RESUME):
            mon.register_callback(self.tool, event, self.tick)
        self.deadline = 0.0
        self.ticks = 0
        self.message = ''

    def tick(self, code, *rest):
        if code.co_filename != USER_FILE:
            return self.disable
        self.ticks += 1
        if not self.ticks & 127 and time.monotonic() > self.deadline:
            self.mon.set_events(self.tool, 0)
            raise TimeoutError(self.message)

    def arm(self, seconds, message):
        self.deadline = time.monotonic() + seconds
        self.message = message
        self.ticks = 0
        self.mon.set_events(self.tool, self.events)

    def disarm(self):
        self.mon.set_events(self.tool, 0)


_guard = None


def _typing_namespace():
    return {
        'ListNode': ListNode, 'TreeNode': TreeNode, 'Node': Node,
        'List': List, 'Optional': Optional, 'Dict': Dict, 'Set': Set,
        'Tuple': Tuple, 'Deque': Deque, 'Any': Any,
    }


def _read(buf):
    # a solution that closes sys.stdout closes the buffer it was redirected to
    try:
        return buf.getvalue()
    except BaseException:
        return ''


def _describe(exc):
    # an exception whose own __str__ raises must not take the run down with it
    try:
        text = str(exc)
    except BaseException:
        text = '<unprintable>'
    return type(exc).__name__ + ': ' + text


def _norm(x):
    return json.dumps(x, sort_keys=True, default=str)


def _approx(actual, expected):
    # bool is an int subclass, so it is told apart first: a bool equals only a bool, and NaN never passes
    if isinstance(actual, bool) or isinstance(expected, bool):
        return type(actual) is type(expected) and actual == expected
    if isinstance(actual, (int, float)) and isinstance(expected, (int, float)):
        return abs(actual - expected) <= APPROX_TOLERANCE
    if isinstance(actual, (list, tuple)) and isinstance(expected, (list, tuple)):
        return len(actual) == len(expected) and all(_approx(a, e) for a, e in zip(actual, expected))
    return actual == expected


def _compare(actual, expected, mode):
    try:
        if mode == 'approx':
            return _approx(actual, expected)
        if mode == 'sorted':
            return sorted(actual) == sorted(expected)
        if mode == 'setOfSorted':
            return [sorted(a) for a in actual] == [sorted(e) for e in expected]
        return _norm(actual) == _norm(expected)
    except Exception:
        return actual == expected


def _build_list(cls, values):
    head = tail = None
    for value in values:
        node = cls(value)
        if head is None:
            head = node
        else:
            tail.next = node
        tail = node
    return head


def _build_tree(cls, values):
    if not values or values[0] is None:
        return None
    root = cls(values[0])
    queue = deque([root])
    i = 1
    while queue and i < len(values):
        node = queue.popleft()
        if values[i] is not None:
            node.left = cls(values[i])
            queue.append(node.left)
        i += 1
        if i < len(values) and values[i] is not None:
            node.right = cls(values[i])
            queue.append(node.right)
        i += 1
    return root


def _build_graph(cls, adjacency):
    # node i has val i + 1 and its neighbours are listed by val (LeetCode's own input form); [] is no node at all
    nodes = [cls(i + 1) for i in range(len(adjacency))]
    for node, neighbours in zip(nodes, adjacency):
        for val in neighbours:
            if isinstance(val, bool) or not isinstance(val, int) or not 1 <= val <= len(nodes):
                raise ValueError('neighbour %r is not a node of the graph (1 to %d)' % (val, len(nodes)))
        node.neighbors = [nodes[val - 1] for val in neighbours]
    return nodes[0] if nodes else None


def _link_cycle(head, pos):
    nodes = []
    while head is not None:
        nodes.append(head)
        head = head.next
    if isinstance(pos, bool) or not isinstance(pos, int) or not -1 <= pos < len(nodes):
        raise ValueError('pos %r is not -1 or an index of the list (%d nodes)' % (pos, len(nodes)))
    if pos >= 0:
        nodes[-1].next = nodes[pos]


def _find(root, val):
    queue = deque([root])
    while queue:
        node = queue.popleft()
        if node is None:
            continue
        if node.val == val:
            return node
        queue.append(node.left)
        queue.append(node.right)
    raise ValueError('no node with value %r in the tree' % (val,))


def _list_values(head):
    out, seen = [], set()
    while head is not None:
        if id(head) in seen or len(out) >= MAX_NODES:
            raise ValueError('the returned linked list has a cycle')
        seen.add(id(head))
        out.append(head.val)
        head = head.next
    return out


def _tree_values(root):
    out, seen, queue = [], set(), deque([root])
    while queue:
        node = queue.popleft()
        if node is None:
            out.append(None)
            continue
        if id(node) in seen or len(out) >= MAX_NODES:
            raise ValueError('the returned tree is not a tree')
        seen.add(id(node))
        out.append(node.val)
        queue.append(node.left)
        queue.append(node.right)
    while out and out[-1] is None:
        out.pop()
    return out


def _to_args(inputs, param_types, ns):
    # the arguments in order and by name: a harness-only key (ListNode_cycle_pos) is no argument, so a name is not a position
    args, named, root, listed, last_list = [], {}, None, False, None
    for name, value in inputs.items():
        kind = param_types.get(name)
        if kind == 'ListNode_cycle_pos':
            if not listed:
                raise ValueError("'%s' links the list before it, and no ListNode parameter comes before it" % name)
            _link_cycle(last_list, value)
            continue
        if kind == 'ListNode':
            value = _build_list(ns['ListNode'], value)
            listed, last_list = True, value
        elif kind == 'ListNode[]':
            value = [_build_list(ns['ListNode'], v) for v in value]
        elif kind == 'TreeNode':
            value = _build_tree(ns['TreeNode'], value)
            root = value if root is None else root
        elif kind == 'TreeNode_by_val':
            value = _find(root, value)
        elif kind == 'GraphNode':
            value = _build_graph(ns['Node'], value)
        args.append(value)
        named[name] = value
    return args, named


def _graph_values(start):
    nodes, seen, queue = [], set(), deque([start])
    while queue:
        node = queue.popleft()
        if id(node) in seen:
            continue
        if len(nodes) >= MAX_NODES:
            raise ValueError('the returned graph is too large')
        seen.add(id(node))
        nodes.append(node)
        queue.extend(node.neighbors)
    vals = [node.val for node in nodes]
    if any(isinstance(v, bool) or not isinstance(v, int) for v in vals) or sorted(vals) != list(range(1, len(vals) + 1)):
        raise ValueError('the returned graph must have the vals 1 to %d, each on one node (it has %r)' % (len(vals), vals))
    by_val = {node.val: node for node in nodes}
    return [[nb.val for nb in by_val[val].neighbors] for val in range(1, len(nodes) + 1)]


def _to_value(kind, value):
    if kind == 'ListNode' and (value is None or hasattr(value, 'next')):
        return _list_values(value)
    if kind == 'GraphNode' and (value is None or hasattr(value, 'neighbors')):
        return [] if value is None else _graph_values(value)
    if kind == 'TreeNode' and (value is None or hasattr(value, 'left')):
        return _tree_values(value)
    if kind == 'TreeNode_val' and value is not None and hasattr(value, 'val'):
        return value.val
    return value


def _jsonable(value):
    try:
        json.dumps(value, default=str)
        return value
    except Exception:
        try:
            return str(value)
        except BaseException:
            return '<unprintable>'


def _scrub(value):
    if isinstance(value, float) and not math.isfinite(value):
        return repr(value)
    if isinstance(value, dict):
        return {k: _scrub(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_scrub(v) for v in value]
    return value


def _dumps(obj):
    try:
        return json.dumps(obj, default=str, allow_nan=False)
    except ValueError:
        return json.dumps(_scrub(obj), default=str, allow_nan=False)


def _char_column(line, byte_offset):
    return len(line.encode('utf-8')[:byte_offset].decode('utf-8', 'ignore')) + 1


def _code_objects(code):
    # the learner's compiled module and every function, class body and lambda nested in it
    found = {code}
    for const in code.co_consts:
        if isinstance(const, type(code)):
            found |= _code_objects(const)
    return found


def _location(exc, lines, owned, compiling):
    # only the learner's own compile reports a position in the learner's file: a SyntaxError raised later, by eval or ast.literal_eval,
    # is about the snippet it was given
    if compiling and isinstance(exc, SyntaxError):
        if exc.lineno is None:
            return None
        at = {'line': exc.lineno}
        if exc.offset:
            at['column'] = exc.offset
        return at
    frame = None
    # `<string>` is every exec's and eval's default filename (dataclasses and namedtuple generate code with it, and a learner's own
    # eval(...) runs in the learner's namespace): the learner's frames are the ones whose code was compiled from the learner's source
    summaries = traceback.extract_tb(exc.__traceback__)
    for summary, (live, _) in zip(summaries, traceback.walk_tb(exc.__traceback__)):
        if summary.lineno and live.f_code in owned:
            frame = summary
    if frame is None:
        return None
    at = {'line': frame.lineno}
    if frame.colno is not None and 0 < frame.lineno <= len(lines):
        at['column'] = _char_column(lines[frame.lineno - 1], frame.colno)
    return at


def _design(cls, inputs, results, step):
    # LeetCode's design shape: operations[0] builds the instance, each later one is a method call, and the answer is every return value
    operations, arguments = inputs.get('operations'), inputs.get('arguments')
    if not isinstance(operations, list) or not isinstance(arguments, list) or not all(isinstance(a, list) for a in arguments):
        raise ValueError("a design case needs a list 'operations' and a list of argument lists 'arguments'")
    if not operations or len(operations) != len(arguments):
        raise ValueError('operations and arguments must have the same length, at least one (%d and %d)' % (len(operations), len(arguments)))
    obj = None
    for index, (name, args) in enumerate(zip(operations, arguments)):
        step[:] = [index, name]
        if index == 0:
            obj = cls(*args)
            results.append(None)
        else:
            results.append(getattr(obj, name)(*args))
    step.clear()


def _reachable_ids(root):
    seen, stack = set(), [root]
    while stack:
        node = stack.pop()
        if node is None or id(node) in seen:
            continue
        seen.add(id(node))
        stack.extend((getattr(node, 'left', None), getattr(node, 'right', None)))
        neighbours = getattr(node, 'neighbors', None)
        if isinstance(neighbours, (list, tuple)):
            stack.extend(neighbours)
    return seen


def _graph_ids(named, param_types):
    ids = set()
    for name, kind in param_types.items():
        if kind == 'GraphNode':
            ids |= _reachable_ids(named.get(name))
    return ids


def _round_trip(cls, harness, args):
    encoded = getattr(cls(), harness['encode'])(*args)
    if not isinstance(encoded, str):
        raise TypeError(harness['encode'] + ' must return a str, got ' + type(encoded).__name__)
    # a new instance, as LeetCode requires the codec to be stateless: one that keeps the input on self fails
    return getattr(cls(), harness['decode'])(encoded)


def _case(cls, method, case, cfg, ns, lines):
    inputs = case.get('inputs', {})
    expected = case.get('expected')
    mode = case.get('compareMode', 'exact')
    out, err = _Capped(), _Capped()
    started = time.perf_counter()
    actual = error = error_at = None
    passed = False
    partial, step = [], []
    harness = cfg['harness']
    kind = harness['kind'] if harness else None
    try:
        if kind == 'design':
            actual = partial
            _guard.arm(cfg['timeout_s'], 'Code execution timed out (' + str(cfg['timeout_s']) + 's limit)')
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                _design(cls, copy.deepcopy(inputs), partial, step)
            _guard.disarm()
        elif kind == 'roundTrip':
            param_kind = next(iter(cfg['paramTypes'].values()), None)
            args, _ = _to_args(copy.deepcopy(inputs), cfg['paramTypes'], ns)
            built = _reachable_ids(args[0]) if param_kind == 'TreeNode' else set()
            _guard.arm(cfg['timeout_s'], 'Code execution timed out (' + str(cfg['timeout_s']) + 's limit)')
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                decoded = _round_trip(cls, harness, args)
            _guard.disarm()
            if built & _reachable_ids(decoded):
                raise ValueError('the returned tree is the input tree, not a decoded copy')
            actual = _to_value(param_kind, decoded)
        else:
            args, named = _to_args(copy.deepcopy(inputs), cfg['paramTypes'], ns)
            given = _graph_ids(named, cfg['paramTypes']) if cfg['returnType'] == 'GraphNode' else set()
            _guard.arm(cfg['timeout_s'], 'Code execution timed out (' + str(cfg['timeout_s']) + 's limit)')
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                returned = getattr(cls(), method)(*args)
            _guard.disarm()
            place = cfg['inPlace']
            if place is None:
                if given and given & _reachable_ids(returned):
                    raise ValueError('the returned graph shares a node with the input: build copies')
                actual = _to_value(cfg['returnType'], returned)
            else:
                actual = _to_value(cfg['paramTypes'].get(place), named[place])
        passed = _compare(actual, expected, mode)
    except BaseException as exc:
        _guard.disarm()
        error = _describe(exc)
        if step:
            error += ' (operation %d: %s)' % tuple(step)
        error_at = _location(exc, lines, cfg['owned'], False)
    row = {
        'pass': passed,
        'input': inputs,
        'actual': _jsonable(actual),
        'expected': expected,
        'stdout': _read(out),
        'stderr': _read(err),
        'error': error,
        'elapsed_ms': round((time.perf_counter() - started) * 1000, 2),
    }
    if error_at:
        row['error_at'] = error_at
    return row


def _failed(kind, message, at=None):
    result = {'ok': False, 'results': [], 'error': message, 'error_kind': kind}
    if at:
        result['error_at'] = at
    return result


def _run(payload, on_case):
    global _guard
    if _guard is None:
        _guard = _Guard()
    code = payload.get('code', '')
    cfg = {
        'paramTypes': payload.get('paramTypes') or {},
        'returnType': payload.get('returnType'),
        'inPlace': payload.get('inPlace'),
        'harness': payload.get('harness'),
        'owned': (),
        'timeout_s': max(1, math.ceil(payload.get('timeoutMs', 5000) / 1000)),
    }
    lines = code.split('\n')
    ns = _typing_namespace()
    boot_out = _Capped()
    compiling = True
    try:
        compiled = compile(code, USER_FILE, 'exec')
        compiling = False
        cfg['owned'] = _code_objects(compiled)
        _guard.arm(cfg['timeout_s'], 'Timeout: module-level code exceeded the ' + str(cfg['timeout_s']) + 's limit')
        with contextlib.redirect_stdout(boot_out):
            exec(compiled, ns)
        _guard.disarm()
    except BaseException as exc:
        _guard.disarm()
        at = _location(exc, lines, cfg['owned'], compiling)
        if isinstance(exc, TimeoutError):
            return _failed('timeout', str(exc), at)
        return _failed('compile', 'CompileError: ' + _describe(exc).split(': ', 1)[1], at)
    class_name = cfg['harness']['className'] if cfg['harness'] else 'Solution'
    cls = ns.get(class_name)
    if cls is None:
        return _failed('compile', 'No ' + class_name + ' class defined')
    results, started = [], time.monotonic()
    for index, case in enumerate(payload.get('testCases', [])):
        row = _case(cls, payload.get('methodName', 'twoSum'), case, cfg, ns, lines)
        if index == 0 and _read(boot_out):
            row['stdout'] = _read(boot_out) + row['stdout']
        results.append(row)
        if on_case is not None:
            try:
                on_case(_dumps({'index': index, 'result': row}))
            except Exception:
                pass
        if time.monotonic() - started >= payload.get('processLimitMs', 30000) / 1000:
            return _failed('timeout', 'Timeout: execution exceeded process limit')
    return {'ok': True, 'results': results}


def run(payload_json, on_case=None):
    saved = (sys.getrecursionlimit(), sys.stdin, sys.stdout, sys.stderr, dict(vars(builtins)))
    set_limit = sys.setrecursionlimit
    sys.setrecursionlimit = lambda n: set_limit(min(n, MAX_RECURSION))
    sys.stdin = io.StringIO('')
    try:
        text = _dumps(_run(json.loads(payload_json), on_case))
        if len(text) > OUTPUT_LIMIT:
            return _dumps(_failed('runtime', 'Runner output exceeded the size limit'))
        return text
    except BaseException as exc:
        return _dumps(_failed('runtime', 'Runner error: ' + type(exc).__name__ + ': ' + str(exc)))
    finally:
        sys.setrecursionlimit = set_limit
        set_limit(saved[0])
        sys.stdin, sys.stdout, sys.stderr = saved[1:4]
        live = vars(builtins)
        for name in [n for n in live if n not in saved[4]]:
            del live[name]
        live.update(saved[4])
