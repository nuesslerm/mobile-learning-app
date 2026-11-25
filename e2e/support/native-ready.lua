local open_library = dofile("e2e/support/native-learner.lua")

run_flow("Given the installed simulator app, when the test update opens, then the learner library is ready", function()
  open_library(device, env.base)
end)
