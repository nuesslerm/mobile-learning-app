
import sys, json, io, contextlib, signal, time, math
from typing import List, Optional, Dict, Set, Tuple, Deque, Any

payload = json.load(sys.stdin)
mode = payload.get('mode', 'run')
code = payload.get('code', '')

def emit(obj):
    sys.stdout.write(json.dumps(obj, default=str))

if mode == 'format':
    formatted = None
    err = None
    try:
        import autopep8
        formatted = autopep8.fix_code(code)
    except Exception:
        try:
            import black
            formatted = black.format_str(code, mode=black.Mode())
        except Exception:
            try:
                import ast
                formatted = ast.unparse(ast.parse(code))
            except Exception as e:
                err = str(e)
    if formatted is None:
        emit({'ok': False, 'formatted': '', 'error': err or 'Format failed'})
    else:
        emit({'ok': True, 'formatted': formatted})
    sys.exit(0)

method_name = payload.get('methodName', 'twoSum')
test_cases = payload.get('testCases', [])
timeout_s = max(1, math.ceil(payload.get('timeoutMs', 5000) / 1000))

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

def norm(x):
    return json.dumps(x, sort_keys=True, default=str)

def compare(actual, expected, cmp):
    try:
        if cmp == 'sorted':
            return sorted(actual) == sorted(expected)
        if cmp == 'setOfSorted':
            return [sorted(a) for a in actual] == [sorted(e) for e in expected]
        return norm(actual) == norm(expected)
    except Exception:
        return actual == expected

class TimeoutError(Exception):
    pass

def _on_alarm(signum, frame):
    raise TimeoutError('Code execution timed out (' + str(timeout_s) + 's limit)')

signal.signal(signal.SIGALRM, _on_alarm)

ns = {
    'ListNode': ListNode, 'TreeNode': TreeNode,
    'List': List, 'Optional': Optional, 'Dict': Dict, 'Set': Set,
    'Tuple': Tuple, 'Deque': Deque, 'Any': Any,
}
try:
    exec(code, ns)
except Exception as e:
    emit({'ok': False, 'results': [], 'error': 'CompileError: ' + str(e)})
    sys.exit(0)

Solution = ns.get('Solution')
if Solution is None:
    emit({'ok': False, 'results': [], 'error': 'No Solution class defined'})
    sys.exit(0)

results = []
for tc in test_cases:
    inputs = tc.get('inputs', {})
    expected = tc.get('expected')
    cmp = tc.get('compareMode', 'exact')
    args = list(inputs.values())
    out, errbuf = io.StringIO(), io.StringIO()
    started = time.time()
    actual = None
    err = None
    passed = False
    try:
        signal.alarm(timeout_s)
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(errbuf):
            sol = Solution()
            actual = getattr(sol, method_name)(*args)
        signal.alarm(0)
        passed = compare(actual, expected, cmp)
    except Exception as e:
        signal.alarm(0)
        err = type(e).__name__ + ': ' + str(e)
    # ensure JSON-serializable actual
    try:
        json.dumps(actual, default=str)
    except Exception:
        actual = str(actual)
    results.append({
        'pass': passed,
        'input': inputs,
        'actual': actual,
        'expected': expected,
        'stdout': out.getvalue(),
        'stderr': errbuf.getvalue(),
        'error': err,
        'elapsed_ms': int((time.time() - started) * 1000),
    })

emit({'ok': True, 'results': results})
