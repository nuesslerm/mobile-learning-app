--[[---
platform: android, browser
against: android=update, browser=export
timeout: 600
focus: true
features: leetcode
---]]
-- JavaScript on the device (issue 86, F1.18): with "This device" chosen, JavaScript runs in a Web Worker of the hidden runner page and the
-- backend is never asked (a stub counts the requests that would reach it). The editor opens in JavaScript through the dev `lang` link
-- parameter. A good solution is Accepted over all 12 cases and a printed line shows in the card; a case that throws shows 11 / 12, the
-- TypeError text and a Line chip; a case that never ends is stopped by the per-case limit and the rest still run; a syntax error shows
-- the compile error; a design (Trie), a tree and a graph problem are Accepted. Waits are on visible elements, never on how fast a run was.
-- wf browser test e2e/js-local-run.lua --env base=http://127.0.0.1:8800/mla-tamagui
-- wf android test e2e/js-local-run.lua --env base=exp://u.expo.dev/<projectId>/group/<group>/--
local android = device.platform == "android"
local base = env.base or (not android and "http://127.0.0.1:8800/mla-tamagui" or nil)
assert_true(base, "pass --env base=<update link>; wf test run does")

local function base64url(s)
  local abc, out = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_", {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = a * 65536 + (b or 0) * 256 + (c or 0)
    for k = 1, 2 + (b and 1 or 0) + (c and 1 or 0) do
      local idx = math.floor(n / 64 ^ (4 - k)) % 64
      out[#out + 1] = abc:sub(idx + 1, idx + 1)
    end
  end
  return table.concat(out)
end

local TWO_SUM = "var twoSum = function(nums, target) {\n  const seen = new Map();\n  for (let i = 0; i < nums.length; i++) {\n%s    const need = target - nums[i];\n    if (seen.has(need)) return [seen.get(need), i];\n    seen.set(nums[i], i);\n  }\n  return [];\n};\n"
local SEEDS = {
  good = TWO_SUM:format(""),
  printing = TWO_SUM:format("    console.log('hello from js');\n"),
  raises = TWO_SUM:format("    if (nums.length === 3 && nums[0] === 3) null.boom;\n"),
  hang = TWO_SUM:format("    if (nums.length === 2 && nums[0] === 3) { while (true) {} }\n"),
  broken = "var twoSum = function(nums, target) {\n  return [;\n};\n",
}

local TRIE = "class Trie {\n  constructor() { this.words = new Set(); }\n  insert(word) { this.words.add(word); }\n  search(word) { return this.words.has(word); }\n  startsWith(prefix) { for (const w of this.words) if (w.startsWith(prefix)) return true; return false; }\n}\n"
local INVERT = "var invertTree = function(root) {\n  if (!root) return null;\n  const left = invertTree(root.left);\n  root.left = invertTree(root.right);\n  root.right = left;\n  return root;\n};\n"
local CLONE = "var cloneGraph = function(node) {\n  if (!node) return null;\n  const copies = new Map();\n  const copy = (n) => {\n    if (copies.has(n)) return copies.get(n);\n    const c = new _Node(n.val);\n    copies.set(n, c);\n    for (const m of n.neighbors) c.neighbors.push(copy(m));\n    return c;\n  };\n  return copy(node);\n};\n"

local function open(path, id, secs)
  local t0 = os.time()
  while os.time() - t0 < (secs or 120) do
    device:open_link(base .. path)
    if device:extended_wait_until{visible={id=id}, timeout=15000, optional=true} then return end
  end
  device:assert_visible({id=id}, {timeout=5000, label='route ready'})
end

local function leave_editor()
  if device:get_element({id='ProblemEditorRunLocalButton'}) then
    device:open_link(base .. "/settings")
    device:assert_not_visible({id='ProblemEditorRunLocalButton'})
  end
end

local function lower_keyboard()
  if android then
    if device:get_device_state('keyboard').visible then
      device:scroll_until_visible{element={id='EditorAction-Done'}, direction='RIGHT', from={id='EditorActionToolbar'}}
      device:tap_on({id='EditorAction-Done'})
    end
    return
  end
  if device:extended_wait_until{visible={id='EditorHelperRails'}, timeout=3000, optional=true} then
    local row, rail = device:get_element({id='ProblemEditorDiffRow'}).bounds, device:get_element({id='EditorHelperRails'}).bounds
    device:tap_on({point=('%d,%d'):format((row.left + row.right) // 2, (row.top + math.min(row.bottom, rail.top - 4)) // 2)})
    device:assert_not_visible({id='EditorHelperRails'})
  end
end

local function open_editor(problem, code)
  leave_editor()
  device:clear_logs()
  open("/leetcode/" .. problem .. "?lang=javascript&seed64=" .. base64url(code), 'ProblemEditorRunLocalButton')
  if not android then
    device:extended_wait_until{condition=function()
      for _, l in ipairs(device:get_logs{level='info'}) do
        if l:find('route.change', 1, true) then return true end
      end
    end, timeout=60000, interval=250, label='the page hydrated'}
  end
  lower_keyboard()
end

local function run_seed(problem, code, expect_text, secs)
  open_editor(problem, code)
  device:tap_on({id='ProblemEditorRunLocalButton'})
  device:extended_wait_until{visible={id=expect_text.id, text=expect_text.text}, timeout=(secs or 120) * 1000, label=problem .. ': ' .. expect_text.label}
end

local function choose_device()
  open("/", 'LeetcodeListRunnerRow')
  if android then
    device:tap_on({id='LeetcodeListRunnerDevice'})
    return
  end
  device:clear_logs()
  local function got_it()
    for _, l in ipairs(device:get_logs{level='info'}) do
      if l:find('leetcode.list.runner_tap', 1, true) then return true end
    end
  end
  for _ = 1, 10 do
    device:tap_on({id='LeetcodeListRunnerDevice'})
    if device:extended_wait_until{condition=got_it, timeout=2000, interval=250, optional=true} then return end
  end
  error("the runner choice never reached the screen")
end

local ACCEPTED = { id = 'ProblemEditorRunOutcomeStatus', text = 'Accepted', label = 'Accepted' }

device:add_network_stub("POST", "*/api/leetcode/run", { body_file = "e2e/stubs/leetcode-run-two-sum.json", headers = { ["content-type"] = "application/json" } })
local ok, failure = pcall(function()
  run_flow("choose this device", function() choose_device() end)

  run_flow("accepted over 12 cases", function()
    run_seed("two-sum", SEEDS.good, { id = 'ProblemEditorRunOutcomeTotals', text = '.*12 / 12.*', label = '12 / 12' }, 240)
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeStatus', text='Accepted'}, timeout=10000, label='Accepted'}
  end)

  run_flow("a printed line shows in the card", function()
    run_seed("two-sum", SEEDS.printing, ACCEPTED)
    device:extended_wait_until{visible={id='ProblemEditorRunCase-0-Stdout', text='.*hello from js.*'}, timeout=10000, label='the printed line'}
  end)

  run_flow("a case throws", function()
    run_seed("two-sum", SEEDS.raises, { id = 'ProblemEditorRunOutcomeTotals', text = '.*11 / 12.*', label = '11 / 12' })
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeRuntimeError', text='.*TypeError.*'}, timeout=10000, label='the TypeError text'}
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeLine', text='Line .*'}, timeout=10000, label='the Line chip'}
  end)

  run_flow("a case never ends", function()
    run_seed("two-sum", SEEDS.hang, { id = 'ProblemEditorRunOutcomeRuntimeError', text = '.*TimeoutError.*', label = 'TimeoutError' }, 120)
    device:extended_wait_until{visible={id='ProblemEditorRunOutcomeTotals', text='.*11 / 12.*'}, timeout=30000, label='the rest still ran'}
  end)

  run_flow("a syntax error", function()
    run_seed("two-sum", SEEDS.broken, { id = 'ProblemEditorRunOutcomeCompileError', text = '.*Error.*', label = 'the compile error' })
  end)

  run_flow("a design problem", function()
    run_seed("implement-trie-prefix-tree", TRIE, ACCEPTED)
  end)

  run_flow("a tree problem", function()
    run_seed("invert-binary-tree", INVERT, ACCEPTED)
  end)

  run_flow("a graph problem", function()
    run_seed("clone-graph", CLONE, ACCEPTED)
  end)

  assert_true(#device:get_network_stub_hits() == 0, "a run reached the backend")
end)
if not ok then error(failure, 0) end
log("js local run: pass")
