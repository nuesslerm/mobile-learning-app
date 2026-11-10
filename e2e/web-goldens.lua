-- Browser goldens: the web export's first screen as a golden, with no JS warning on the way.
-- wf browser test e2e/web-goldens.lua --env base=http://127.0.0.1:8080 [--env ready=<testID>] [--update-goldens]
-- Serve a local `pnpm --filter mla-tamagui run export:web` (dist/) at base; goldens land in docs/runbooks/goldens/browser/.
local base = env.base or "http://127.0.0.1:8080"
local ready = {id=env.ready or 'DebugFab'}

device:clear_logs()
device:open_link(base .. "/")
device:assert_visible(ready, {timeout=60000})
local home = device:assert_screenshot("web-home")
log(json.encode({ golden = "web-home", verdict = home.verdict, match_percentage = home.match_percentage }))

local warnings = device:get_logs()
for _, w in ipairs(warnings) do log(w) end
assert_true(#warnings == 0, "the first screen logged " .. #warnings .. " JS warnings")
