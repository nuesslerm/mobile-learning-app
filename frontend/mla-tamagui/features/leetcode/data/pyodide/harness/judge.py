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
        'ListNode': ListNode, 'TreeNode': TreeNode,
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
    args, root = [], None
    for name, value in inputs.items():
        kind = param_types.get(name)
        if kind == 'ListNode':
            value = _build_list(ns['ListNode'], value)
        elif kind == 'ListNode[]':
            value = [_build_list(ns['ListNode'], v) for v in value]
        elif kind == 'TreeNode':
            value = _build_tree(ns['TreeNode'], value)
            root = value if root is None else root
        elif kind == 'TreeNode_by_val':
            value = _find(root, value)
        args.append(value)
    return args


def _to_value(kind, value):
    if kind == 'ListNode' and (value is None or hasattr(value, 'next')):
        return _list_values(value)
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


def _case(solution, method, case, cfg, ns, lines):
    inputs = case.get('inputs', {})
    expected = case.get('expected')
    mode = case.get('compareMode', 'exact')
    out, err = _Capped(), _Capped()
    started = time.perf_counter()
    actual = error = error_at = None
    passed = False
    try:
        args = _to_args(copy.deepcopy(inputs), cfg['paramTypes'], ns)
        _guard.arm(cfg['timeout_s'], 'Code execution timed out (' + str(cfg['timeout_s']) + 's limit)')
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            returned = getattr(solution(), method)(*args)
        _guard.disarm()
        place = cfg['inPlace']
        if place is None:
            actual = _to_value(cfg['returnType'], returned)
        else:
            actual = _to_value(cfg['paramTypes'].get(place), args[list(inputs).index(place)])
        passed = _compare(actual, expected, mode)
    except BaseException as exc:
        _guard.disarm()
        error = _describe(exc)
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
    solution = ns.get('Solution')
    if solution is None:
        return _failed('compile', 'No Solution class defined')
    results, started = [], time.monotonic()
    for index, case in enumerate(payload.get('testCases', [])):
        row = _case(solution, payload.get('methodName', 'twoSum'), case, cfg, ns, lines)
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
