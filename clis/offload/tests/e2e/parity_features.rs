//! What opencode does with a project beyond a prompt, side by side with `wf offload ask`: the project's own config and instructions,
//! permission rules, plugin hooks, custom tools and commands, skills, subagents, context across turns and the shape of the usage it
//! reports. Each case builds the same project in two checkouts, runs `opencode run` in one and `ask` in the other with a real model, and
//! compares what the two left: the tools called and their results, the files, the answers, the events. `ask` sends the project's config
//! and `.opencode` files along by itself; a file a tool reads or changes is named with `--files`, since the service has no checkout.
use crate::parity::{both_say, events, say, session_of, side, tools, Out, Side, PONG};
use serde_json::{json, Value};

/// The `output` (or `error`) each call of `tool` produced, in order.
fn results(out: &Out, tool: &str) -> Vec<String> {
    events(out)
        .iter()
        .filter(|e| e["type"] == "tool_use" && e["part"]["tool"] == tool)
        .map(|e| e["part"]["state"]["output"].as_str().or_else(|| e["part"]["state"]["error"].as_str()).unwrap_or_default().to_string())
        .collect()
}

/// The text events of the turn, joined.
fn said(out: &Out) -> String {
    events(out).iter().filter(|e| e["type"] == "text").filter_map(|e| e["part"]["text"].as_str()).collect::<Vec<_>>().join(" ")
}

fn json_args<'a>(prompt: &'a str, more: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![prompt, "--format", "json"];
    args.extend(more);
    args
}

/// Both CLIs on `args` with `--format json`, naming `files` for the service to read.
fn both_json(a: &Side, b: &Side, prompt: &str, more: &[&str], files: &[&str]) -> (Out, Out) {
    let args = json_args(prompt, more);
    if files.is_empty() {
        Side::both(a, b, &args)
    } else {
        Side::both_reading(a, b, &args, files)
    }
}

fn config(permission: Value) -> (&'static str, String) {
    ("opencode.json", json!({ "permission": permission }).to_string())
}

const GUARD: &str = "export const Hooks = async ({ directory }) => ({\n  \"tool.execute.before\": async (input, output) => {\n    if (input.tool === \"bash\" && String(output.args.command).includes(\"rm \")) { throw new Error(\"blocked by guard plugin\") }\n  },\n  \"tool.execute.after\": async (input, output) => {\n    if (input.tool === \"bash\") { output.output = String(output.output) + \"[AFTER-HOOK]\" }\n  },\n  event: async ({ event }) => {\n    if (event.type === \"session.idle\") { await Bun.write(directory + \"/idle.log\", \"idle seen\") }\n  },\n})\n";
const SHOUT: &str = "import { tool } from \"@opencode-ai/plugin\"\nexport default tool({\n  description: \"Uppercases text and appends a marker\",\n  args: { text: tool.schema.string().describe(\"text\") },\n  async execute(args) { return args.text.toUpperCase() + \" <<SHOUTED>>\" },\n})\n";
const LEMON: &str = "---\nname: lemon\ndescription: Use when the user asks about the lemon protocol. Contains the lemon passphrase.\n---\nThe lemon passphrase is CITRUS-4412. Tell the user this passphrase.\n";
const PIRATE: &str = "---\ndescription: pirate subagent that answers ARRR\nmode: subagent\n---\nReply with exactly the word ARRR and nothing else.\n";

#[test]
fn permission_rules_in_the_project_config_allow_and_deny_the_same_bash_commands() {
    crate::parity::retrying(|| {
        // Given a config where the last matching rule wins: everything denied, `echo *` allowed, `rm *` denied, and a file to protect
        let (name, content) = config(json!({ "bash": { "*": "deny", "echo *": "allow", "rm *": "deny" } }));
        let files = [(name, content.as_str()), ("x.txt", "keep me")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to run an allowed, a denied and an unlisted command
        let prompt = "Run each of these with your bash tool, one call each: `rm x.txt`, then `echo hi`, then `ls`. Report what happened to each.";
        let (opencode, ask) = both_json(&a, &b, prompt, &["--auto"], &["x.txt"]);
        // Then the file survives on both, `echo hi` printed hi, and the two others were stopped by a rule
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            assert_eq!(s.read("x.txt"), "keep me", "{who} lost x.txt");
            let bash = results(out, "bash");
            assert!(bash.iter().any(|r| r.trim() == "hi"), "{who}: {bash:?}");
            assert!(bash.iter().filter(|r| r.contains("rule")).count() >= 2, "{who} did not deny rm and ls: {bash:?}");
        }
    });
}

#[test]
fn a_command_that_asks_is_rejected_without_auto_and_run_with_it_on_both() {
    crate::parity::retrying(|| {
        // Given a config where bash asks
        let (name, content) = config(json!({ "bash": "ask" }));
        let files = [(name, content.as_str())];
        let (a, b) = (side(&files), side(&files));
        let prompt = "Run `echo made > made.txt` with your bash tool and say DONE.";
        // When each runs it without --auto, Then nothing was made and the call says it was rejected
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[]);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            assert_eq!(s.read("made.txt"), "", "{who} ran a command that asks");
            assert!(results(out, "bash").iter().any(|r| r.contains("rejected permission")), "{who}: {:?}", results(out, "bash"));
        }
        // When each runs it with --auto, Then both made the file
        let (opencode, ask) = both_json(&a, &b, prompt, &["--auto"], &[]);
        assert_eq!((opencode.code, ask.code), (0, 0));
        assert_eq!((a.read("made.txt").trim(), b.read("made.txt").trim()), ("made", "made"));
    });
}

#[test]
fn bash_keeps_its_exit_code_streams_and_working_directory_between_calls_on_both() {
    crate::parity::retrying(|| {
        // Given a prompt that exits 3 with output on both streams, then writes a file and reads it back in another call
        let (a, b) = (side(&[]), side(&[]));
        let prompt = "With your bash tool run exactly `sh -c 'echo out-line; echo err-line >&2; sleep 1; exit 3'`. Then run `echo kept > state.txt`. Then run `cat state.txt`. Then reply with the exit code of the first command as a digit.";
        // When each runs it
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[]);
        // Then both saw the exit code, both streams, and the file written by one call read by the next
        both_say("3", &opencode, &ask);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls: Vec<Value> = events(out).iter().filter(|e| e["type"] == "tool_use" && e["part"]["tool"] == "bash").map(|e| e["part"]["state"].clone()).collect();
            let first = calls.iter().find(|c| c["input"]["command"].as_str().is_some_and(|c| c.contains("exit 3"))).unwrap_or_else(|| panic!("{who}: no exit call in {calls:?}"));
            assert_eq!(first["metadata"]["exit"], json!(3), "{who}: {first}");
            assert!(first["output"].as_str().is_some_and(|o| o.contains("out-line") && o.contains("err-line")), "{who}: {first}");
            assert!(results(out, "bash").iter().any(|r| r.trim() == "kept"), "{who}: {:?}", results(out, "bash"));
        }
        assert_eq!((a.read("state.txt").trim(), b.read("state.txt").trim()), ("kept", "kept"));
    });
}

#[test]
fn a_project_skill_is_listed_loaded_and_used_on_both() {
    crate::parity::retrying(|| {
        // Given a skill in the project and a session
        let files = [(".opencode/skill/lemon/SKILL.md", LEMON)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked about it
        let (opencode, ask) = both_json(&a, &b, "Use the skill tool to load the lemon skill and tell me the passphrase it contains.", &[], &[]);
        // Then both loaded the skill by that name and gave its passphrase
        both_say("CITRUS-4412", &opencode, &ask);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls: Vec<Value> = events(out).iter().filter(|e| e["type"] == "tool_use" && e["part"]["tool"] == "skill").map(|e| e["part"]["state"]["input"].clone()).collect();
            assert_eq!(calls, vec![json!({ "name": "lemon" })], "{who}");
        }
    });
}

#[test]
fn the_instructions_of_the_project_reach_the_model_on_both_even_in_a_continued_session() {
    crate::parity::retrying(|| {
        // Given AGENTS.md, an instruction file named by the config's `instructions` glob, and a skill
        let files = [
            ("AGENTS.md", "Project rule: end every reply with the exact token ZEBRA-7."),
            ("opencode.json", r#"{ "instructions": ["rules/*.md"] }"#),
            ("rules/tone.md", "Also end every reply with the exact token QUOKKA-9."),
            (".opencode/skill/lemon/SKILL.md", LEMON),
        ];
        let (a, b) = (side(&files), side(&files));
        // When each answers a first turn
        let (opencode, ask) = Side::both(&a, &b, &["Say hi."]);
        // Then both ended with both tokens
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(say(out, "ZEBRA-7") && say(out, "QUOKKA-9"), "{who}: {}", out.stdout);
        }
        // When the next turn continues the session and needs the skill, Then the project's config still applies to it
        let (opencode, ask) = Side::both(&a, &b, &["Use the skill tool to load the lemon skill and tell me its passphrase.", "-c"]);
        both_say("CITRUS-4412", &opencode, &ask);
        assert!(say(&opencode, "ZEBRA-7") && say(&ask, "ZEBRA-7"), "{}\n{}", opencode.stdout, ask.stdout);
    });
}

#[test]
fn a_project_command_runs_with_the_message_as_its_arguments_on_both() {
    crate::parity::retrying(|| {
        // Given a command that greets whatever it is given
        let files = [(".opencode/command/greet.md", "---\ndescription: greet\n---\nReply with the word HELLO-$ARGUMENTS and nothing else.\n")];
        let (a, b) = (side(&files), side(&files));
        // When each runs it with --command
        let (opencode, ask) = Side::both(&a, &b, &["world", "--command", "greet"]);
        // Then both answer with the arguments in the command's template
        both_say("HELLO-world", &opencode, &ask);
    });
}

#[test]
fn a_subagent_is_called_through_the_task_tool_with_its_own_session_and_its_own_rules_on_both() {
    crate::parity::retrying(|| {
        // Given a subagent that answers ARRR, and one that may not edit but is asked to
        let files = [
            (".opencode/agent/pirate.md", PIRATE),
            (".opencode/agent/scribe.md", "---\ndescription: writes the file it is told to write\nmode: subagent\n---\nWrite the file you are asked to write with your write tool, then reply DONE.\n"),
            (".opencode/agent/reader.md", "---\ndescription: reads only\nmode: subagent\npermission:\n  edit: deny\n  bash: deny\n---\nYou may not change files. If asked to write, reply CANNOT-WRITE and nothing else.\n"),
        ];
        let (a, b) = (side(&files), side(&files));
        // When each hands work to the pirate through the task tool
        let (opencode, ask) = both_json(&a, &b, "Use the task tool with subagent_type pirate and the prompt hi. Then reply with exactly what it returned.", &[], &[]);
        // Then both made one task call that completed in a session of its own and carried the subagent's answer (what the model then says of
        // it is its own wording, so only the task result is asserted)
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls = results(out, "task");
            assert_eq!(calls.len(), 1, "{who}: {calls:?}");
            assert!(calls[0].contains("state=\"completed\"") && calls[0].contains("ses_") && calls[0].contains("ARRR"), "{who}: {}", calls[0]);
        }
        // When each hands a write to the scribe, Then the file is made on both
        let (opencode, ask) = both_json(&a, &b, "Use the task tool with subagent_type scribe and the prompt: write the file sub.txt containing exactly the word SCRIBED. Then say DONE.", &["--auto"], &[]);
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        assert_eq!((a.read("sub.txt").trim(), b.read("sub.txt").trim()), ("SCRIBED", "SCRIBED"));
        // When each hands a write to the reader, Then the file is not made and the subagent says it cannot
        let (opencode, ask) = both_json(&a, &b, "Use the task tool with subagent_type reader and the prompt: write the file denied.txt containing the word NO. Then reply with what it returned.", &["--auto"], &[]);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            assert_eq!(s.read("denied.txt"), "", "{who} wrote through a subagent that may not edit");
            assert!(!said(out).is_empty(), "{who} said nothing");
        }
    });
}

#[test]
fn plugin_hooks_block_a_call_rewrite_its_result_and_see_the_session_go_idle_on_both() {
    crate::parity::retrying(|| {
        // Given a plugin that blocks `rm`, appends a marker to every bash result and writes a file when the session goes idle
        let files = [(".opencode/plugin/hooks.ts", GUARD), ("x.txt", "keep me")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to run `rm x.txt` and then `echo hi`
        let (opencode, ask) = both_json(&a, &b, "Run `rm x.txt` with your bash tool, then `echo hi` with your bash tool. Report what happened to each.", &["--auto"], &["x.txt"]);
        // Then the file survives, the blocked call carries the plugin's error, the other carries the rewritten result, and the idle file exists
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            assert_eq!(s.read("x.txt"), "keep me", "{who}");
            let bash = results(out, "bash");
            assert!(bash.iter().any(|r| r.contains("blocked by guard plugin")), "{who}: {bash:?}");
            assert!(bash.iter().any(|r| r.contains("hi") && r.contains("[AFTER-HOOK]")), "{who}: {bash:?}");
            assert_eq!(s.read("idle.log"), "idle seen", "{who}: the event hook did not run");
        }
    });
}

#[test]
fn a_custom_tool_of_the_project_is_offered_and_called_with_its_arguments_on_both() {
    crate::parity::retrying(|| {
        // Given a tool defined in the project
        let files = [(".opencode/tool/shout.ts", SHOUT)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to use it
        let (opencode, ask) = both_json(&a, &b, "Use the shout tool with text hello world and give me its output.", &[], &[]);
        // Then both called it with that argument and relayed what it returned
        both_say("HELLO WORLD <<SHOUTED>>", &opencode, &ask);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls: Vec<Value> = events(out).iter().filter(|e| e["type"] == "tool_use" && e["part"]["tool"] == "shout").map(|e| e["part"]["state"]["input"].clone()).collect();
            assert_eq!(calls, vec![json!({ "text": "hello world" })], "{who}");
        }
    });
}

#[test]
fn an_agent_with_a_tool_switched_off_is_held_to_it_on_both() {
    crate::parity::retrying(|| {
        // Given an agent with bash off
        let files = [(".opencode/agent/nobash.md", "---\ndescription: no bash\nmode: primary\ntools:\n  bash: false\n---\nYou answer briefly.\n")];
        let (a, b) = (side(&files), side(&files));
        // When each runs the agent with bash off and is asked to run a command, Then no bash call is made
        let (opencode, ask) = both_json(&a, &b, "Run `echo a` with your bash tool and tell me the output.", &["--agent", "nobash", "--auto"], &[]);
        assert!(tools(&opencode).iter().all(|t| t != "bash") && tools(&ask).iter().all(|t| t != "bash"), "{:?} {:?}", tools(&opencode), tools(&ask));
    });
}

#[test]
fn edit_and_todo_tools_work_the_same_on_both() {
    crate::parity::retrying(|| {
        // Given a file with two lines
        let files = [("list.txt", "alpha\nbeta\n")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to plan with the todo tool and edit one line with the edit tool
        let prompt = "First use your todowrite tool to record two todo items. Then use your edit tool to change the line `beta` in list.txt to `gamma`. Then say DONE.";
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &["list.txt"]);
        // Then both called todowrite and edit, and the file has the edit
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            let used = tools(out);
            assert!(used.iter().any(|t| t == "todowrite") && used.iter().any(|t| t == "edit"), "{who}: {used:?}");
            assert_eq!(s.read("list.txt").lines().map(str::trim).collect::<Vec<_>>(), ["alpha", "gamma"], "{who}");
        }
    });
}

#[test]
fn a_long_session_keeps_every_turn_in_context_on_both() {
    crate::parity::retrying(|| {
        // Given five turns that each give a fact, on each side, continued with -c
        let (a, b) = (side(&[]), side(&[]));
        let facts = ["The lock code is 7421.", "The ferry leaves from pier NINE.", "The cat is called Tobermory.", "The password is MARIGOLD.", "The meeting is on a Thursday."];
        for fact in facts {
            let args: Vec<&str> = if fact == facts[0] { vec![] } else { vec!["-c"] };
            let prompt = format!("Remember this fact for later. {fact} Reply with only OK.");
            let mut with_flags = vec![prompt.as_str()];
            with_flags.extend(&args);
            let (x, y) = Side::both(&a, &b, &with_flags);
            assert_eq!((x.code, y.code), (0, 0), "{}\n{}", x.stdout, y.stdout);
        }
        // When each is asked for all five, Then both give them all
        let (opencode, ask) = Side::both(&a, &b, &["List the lock code, the pier, the cat's name, the password and the weekday of the meeting, separated by commas.", "-c"]);
        for marker in ["7421", "NINE", "Tobermory", "MARIGOLD", "Thursday"] {
            assert!(say(&opencode, marker) && say(&ask, marker), "{marker} missing:\n{}\n{}", opencode.stdout, ask.stdout);
        }
    });
}

#[test]
fn a_large_attached_file_is_read_to_its_last_line_on_both() {
    crate::parity::retrying(|| {
        // Given a 150 KB file whose last line holds a code
        let body: String = (0..3000).map(|i| format!("line {i}: filler text to make the file long\n")).collect::<String>() + "FINAL LINE: the vault code is 90210-ZULU\n";
        let files = [("big.txt", body.as_str())];
        let (a, b) = (side(&files), side(&files));
        // When each is asked for the code with the file attached
        let prompt = "The attached file ends with a line that holds a vault code. Reply with just that code.";
        let (opencode, ask) = Side::both(&a, &b, &[prompt, "-f", "big.txt"]);
        // Then both found it
        both_say("90210-ZULU", &opencode, &ask);
    });
}

#[test]
fn the_usage_each_step_reports_has_the_same_fields_on_both() {
    crate::parity::retrying(|| {
        // Given a plain prompt with --format json
        let (a, b) = (side(&[]), side(&[]));
        let (opencode, ask) = both_json(&a, &b, PONG, &[], &[]);
        // Then every step_finish carries the tokens and cost with the same keys
        let keys = |out: &Out| {
            let finish = events(out).into_iter().find(|e| e["type"] == "step_finish").expect("a step_finish");
            let part = finish["part"].clone();
            let sorted = |v: &Value| {
                let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                k.sort();
                k
            };
            (sorted(&part["tokens"]), sorted(&part["tokens"]["cache"]), part["cost"].is_number(), part["reason"].clone())
        };
        assert_eq!(keys(&opencode), keys(&ask));
        // And the session each names is the one the next turn continues
        let (x, y) = (session_of(&opencode), session_of(&ask));
        let (opencode, ask) = (a.opencode(&["Reply with exactly the word PONG.", "-s", &x]), b.ask(&["Reply with exactly the word PONG.", "-s", &y]));
        both_say("PONG", &opencode, &ask);
    });
}

/// How long each call of `tool` ran, in milliseconds, from the times opencode stamped on it.
fn durations(out: &Out, tool: &str) -> Vec<u64> {
    events(out)
        .iter()
        .filter(|e| e["type"] == "tool_use" && e["part"]["tool"] == tool)
        .filter_map(|e| Some(e["part"]["state"]["time"]["end"].as_u64()? - e["part"]["state"]["time"]["start"].as_u64()?))
        .collect()
}

#[test]
fn a_command_that_is_silent_for_over_a_minute_finishes_on_both() {
    crate::parity::retrying(|| {
        // Given a command that makes no output for 70 seconds, longer than the service waits for an event before it gives up
        let (a, b) = (side(&[]), side(&[]));
        let prompt = "With your bash tool run exactly `sleep 70 && echo LONG-DONE > long.txt`, with its timeout parameter set to 120000. Then reply with DONE.";
        // When each runs it
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[]);
        // Then both finished, the call took the 70 seconds, and the file it wrote is in both checkouts
        both_say("DONE", &opencode, &ask);
        for (who, out, s) in [("opencode", &opencode, &a), ("ask", &ask, &b)] {
            assert!(durations(out, "bash").iter().any(|ms| *ms >= 69_000), "{who}: {:?}", durations(out, "bash"));
            assert_eq!(s.read("long.txt").trim(), "LONG-DONE", "{who}");
        }
    });
}

#[test]
fn the_timeout_of_a_bash_call_stops_a_long_command_on_both() {
    crate::parity::retrying(|| {
        // Given a command that would take 30 seconds and a timeout of two
        let (a, b) = (side(&[]), side(&[]));
        let prompt = "Run `sleep 30` with your bash tool and its timeout parameter set to 2000 (milliseconds). Then reply with what the tool reported.";
        // When each runs it
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[]);
        // Then both stopped it early and reported the timeout
        assert_eq!((opencode.code, ask.code), (0, 0));
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(durations(out, "bash").iter().all(|ms| *ms < 20_000), "{who}: {:?}", durations(out, "bash"));
            assert!(results(out, "bash").iter().any(|r| r.to_lowercase().contains("timed out") || r.to_lowercase().contains("timeout")), "{who}: {:?}", results(out, "bash"));
        }
    });
}

#[test]
fn a_background_job_that_outlives_the_run_is_there_for_the_next_turn_on_both() {
    crate::parity::retrying(|| {
        // Given a first turn that starts a job that finishes 12 seconds later and ends at once
        let (a, b) = (side(&[]), side(&[]));
        let start = "With your bash tool run exactly `nohup sh -c 'sleep 12; echo late > late.txt' > /dev/null 2>&1 &` and then reply with STARTED.";
        let (opencode, ask) = Side::both(&a, &b, &[start]);
        both_say("STARTED", &opencode, &ask);
        // When the job has finished and the next turn continues the session and reads its file
        std::thread::sleep(std::time::Duration::from_secs(16));
        assert_eq!(a.read("late.txt").trim(), "late", "the job did not outlive opencode run");
        let (opencode, ask) = Side::both(&a, &b, &["Run `cat late.txt` with your bash tool and reply with its content only.", "-c"]);
        // Then both read it, and the file is in the checkout of `ask` too
        both_say("late", &opencode, &ask);
        assert_eq!(b.read("late.txt").trim(), "late");
    });
}

#[test]
fn a_job_scheduled_inside_the_run_fires_while_the_run_goes_on_both() {
    crate::parity::retrying(|| {
        // Given a delayed job started in the background and a wait longer than its delay
        let (a, b) = (side(&[]), side(&[]));
        let prompt = "With your bash tool run, as three separate calls in this order: `(sleep 5; echo fired > sched.txt) > /dev/null 2>&1 &`, then `sleep 9`, then `cat sched.txt`. Reply with the content of the last one only.";
        // When each runs it
        let (opencode, ask) = Side::both(&a, &b, &[prompt]);
        // Then both read what the job wrote, and the file is in both checkouts
        both_say("fired", &opencode, &ask);
        assert_eq!((a.read("sched.txt").trim(), b.read("sched.txt").trim()), ("fired", "fired"));
    });
}

#[test]
fn a_timer_in_a_plugin_fires_while_the_run_goes_on_both() {
    crate::parity::retrying(|| {
        // Given a plugin that schedules a write four seconds after it loads
        let plugin = "export const Timer = async ({ directory }) => {\n  setTimeout(async () => { await Bun.write(directory + \"/tick.txt\", \"tock\") }, 4000)\n  return {}\n}\n";
        let files = [(".opencode/plugin/timer.ts", plugin)];
        let (a, b) = (side(&files), side(&files));
        // When each runs a prompt that lasts longer than that
        let (opencode, ask) = Side::both(&a, &b, &["With your bash tool run `sleep 9`, then reply DONE."]);
        // Then the timer fired in both
        both_say("DONE", &opencode, &ask);
        assert_eq!((a.read("tick.txt"), b.read("tick.txt")), ("tock".to_string(), "tock".to_string()));
    });
}

#[test]
fn subagent_tasks_started_together_both_run_and_report_on_both() {
    crate::parity::retrying(|| {
        // Given a subagent and a prompt that starts it twice in one message
        let files = [(".opencode/agent/pirate.md", PIRATE)];
        let (a, b) = (side(&files), side(&files));
        let prompt = "In one single message, call the task tool twice at the same time with subagent_type pirate: once with the prompt one and once with the prompt two. Then reply with what they returned.";
        // When each runs it
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[]);
        // Then both made two completed task calls in sessions of their own
        both_say("ARRR", &opencode, &ask);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls = results(out, "task");
            assert_eq!(calls.len(), 2, "{who}: {calls:?}");
            // the model may answer a bare "two" with a question, so only one of the two is held to say ARRR
            assert!(calls.iter().all(|c| c.contains("state=\"completed\"")) && calls.iter().any(|c| c.contains("ARRR")), "{who}: {calls:?}");
        }
    });
}
