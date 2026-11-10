//! The capabilities `opencode` documents (opencode.ai/docs: config, rules, agents, commands, permissions, skills, tools, custom tools,
//! plugins, MCP servers, formatters, models, sessions), each exercised through `opencode run` and through `wf offload ask` on the same
//! project with a real model, and the two compared. A feature of the CLI that has no counterpart on the gateway fails here.
use crate::parity::{both_say, events, say, session_of, side, tools, Out, Side};
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

/// Both CLIs with `--format json`, `files` named for the service to read (it has no checkout).
fn both_json(a: &Side, b: &Side, prompt: &str, more: &[&str], files: &[&str]) -> (Out, Out) {
    let mut args = vec![prompt, "--format", "json"];
    args.extend(more);
    if files.is_empty() {
        Side::both(a, b, &args)
    } else {
        Side::both_reading(a, b, &args, files)
    }
}

fn config(value: Value) -> String {
    value.to_string()
}

const PRIMARY_ARRR: &str = "---\ndescription: answers ARRR\nmode: primary\n---\nWhatever you are asked, reply with exactly the word ARRR and nothing else.\n";
const SUB_PIRATE: &str = "---\ndescription: pirate subagent that answers ARRR\nmode: subagent\n---\nReply with exactly the word ARRR and nothing else.\n";

// ---- rules, skills, config ----

#[test]
fn skills_in_the_claude_and_agents_directories_are_found_like_those_of_opencode() {
    crate::parity::retrying(|| {
        // Given a skill under .claude/skills and another under .agents/skills
        let files = [
            (".claude/skills/amber/SKILL.md", "---\nname: amber\ndescription: Use when asked for the amber passphrase.\n---\nThe amber passphrase is AMBER-11.\n"),
            (".agents/skills/slate/SKILL.md", "---\nname: slate\ndescription: Use when asked for the slate passphrase.\n---\nThe slate passphrase is SLATE-22.\n"),
        ];
        let (a, b) = (side(&files), side(&files));
        // When each is asked for both passphrases
        let (opencode, ask) = both_json(&a, &b, "Use the skill tool to load the amber skill and the slate skill, then give me both passphrases separated by a comma.", &[], &[]);
        // Then both loaded both skills and gave both
        for marker in ["AMBER-11", "SLATE-22"] {
            assert!(say(&opencode, marker) && say(&ask, marker), "{marker} missing:\n{}\n{}", opencode.stdout, ask.stdout);
        }
    });
}

#[test]
fn claude_md_is_the_rules_file_only_when_there_is_no_agents_md_on_both() {
    crate::parity::retrying(|| {
        // Given a project with CLAUDE.md alone, and one with both files
        let alone = [("CLAUDE.md", "End every reply with the exact token ZEBRA-7.")];
        let both = [("AGENTS.md", "End every reply with the exact token QUOKKA-9."), ("CLAUDE.md", "End every reply with the exact token ZEBRA-7.")];
        let (a, b) = (side(&alone), side(&alone));
        let (c, d) = (side(&both), side(&both));
        // When each answers a greeting
        let (opencode, ask) = Side::both(&a, &b, &["Say hi."]);
        let (opencode_both, ask_both) = Side::both(&c, &d, &["Say hi."]);
        // Then CLAUDE.md applied when alone, and with AGENTS.md beside it only AGENTS.md did
        assert!(say(&opencode, "ZEBRA-7") && say(&ask, "ZEBRA-7"), "{}\n{}", opencode.stdout, ask.stdout);
        assert!(say(&opencode_both, "QUOKKA-9") && say(&ask_both, "QUOKKA-9"), "{}\n{}", opencode_both.stdout, ask_both.stdout);
        assert!(!say(&opencode_both, "ZEBRA-7") && !say(&ask_both, "ZEBRA-7"), "{}\n{}", opencode_both.stdout, ask_both.stdout);
    });
}

#[test]
fn the_default_agent_of_the_config_answers_when_no_agent_is_named_on_both() {
    crate::parity::retrying(|| {
        // Given a primary agent and a config that makes it the default
        let cfg = config(json!({ "default_agent": "arrr" }));
        let files = [("opencode.json", cfg.as_str()), (".opencode/agent/arrr.md", PRIMARY_ARRR)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked without --agent
        let (opencode, ask) = Side::both(&a, &b, &["Say hello."]);
        // Then both answered as that agent
        both_say("ARRR", &opencode, &ask);
    });
}

#[test]
fn an_agent_whose_prompt_is_a_file_reference_in_the_config_answers_from_that_file_on_both() {
    crate::parity::retrying(|| {
        // Given an agent in the config whose prompt is {file:...}
        let cfg = config(json!({ "agent": { "filed": { "mode": "primary", "description": "prompt from a file", "prompt": "{file:./prompts/filed.md}" } } }));
        let files = [("opencode.json", cfg.as_str()), ("prompts/filed.md", "Whatever you are asked, reply with exactly the word FILED-PROMPT and nothing else.")];
        let (a, b) = (side(&files), side(&files));
        // When each runs it
        let (opencode, ask) = Side::both(&a, &b, &["Say hello.", "--agent", "filed"]);
        // Then both used the file's text
        both_say("FILED-PROMPT", &opencode, &ask);
    });
}

// ---- commands ----

#[test]
fn command_arguments_are_split_into_positional_parameters_on_both() {
    crate::parity::retrying(|| {
        // Given a command that uses $1, $2 and $3
        let files = [(".opencode/command/join.md", "---\ndescription: joins\n---\nReply with exactly this text and nothing else: $1+$2+$3\n")];
        let (a, b) = (side(&files), side(&files));
        // When each runs it with three plain arguments, Then both substituted them
        let (opencode, ask) = Side::both(&a, &b, &["alpha", "beta", "gamma", "--command", "join"]);
        both_say("alpha+beta+gamma", &opencode, &ask);
        // When each runs it with a quoted argument, Then both split it the same way, whatever that is
        let (opencode, ask) = Side::both(&a, &b, &["alpha beta \"gamma delta\"", "--command", "join"]);
        assert_eq!((opencode.code, ask.code), (0, 0));
        assert_eq!(opencode.stdout.trim(), ask.stdout.trim());
    });
}

#[test]
fn a_command_runs_shell_output_into_its_prompt_and_reads_referenced_files_on_both() {
    crate::parity::retrying(|| {
        // Given a command with !`...` shell output and an @file reference
        let files = [
            (".opencode/command/probe.md", "---\ndescription: probe\n---\nThe shell said: !`echo SHELL-INJECTED-42`\nThe file @data/code.txt holds a code. Reply with the exact token the shell printed (the one in capital letters and digits), a space, and the code from the file, and nothing else.\n"),
            ("data/code.txt", "code: 5150-ALPHA"),
        ];
        let (a, b) = (side(&files), side(&files));
        // When each runs it
        let (opencode, ask) = Side::both(&a, &b, &["--command", "probe"]);
        // Then both saw the output of the shell command and the file
        for marker in ["SHELL-INJECTED-42", "5150-ALPHA"] {
            assert!(say(&opencode, marker) && say(&ask, marker), "{marker} missing:\n{}\n{}", opencode.stdout, ask.stdout);
        }
    });
}

#[test]
fn a_command_defined_in_the_config_with_an_agent_runs_as_that_agent_on_both() {
    crate::parity::retrying(|| {
        // Given a command in opencode.json that names a primary agent
        let cfg = config(json!({ "command": { "hello": { "template": "Greet $ARGUMENTS.", "description": "greets", "agent": "arrr" } } }));
        let files = [("opencode.json", cfg.as_str()), (".opencode/agent/arrr.md", PRIMARY_ARRR)];
        let (a, b) = (side(&files), side(&files));
        // When each runs it
        let (opencode, ask) = Side::both(&a, &b, &["world", "--command", "hello"]);
        // Then both ran it as that agent
        both_say("ARRR", &opencode, &ask);
    });
}

// ---- permissions ----

#[test]
fn edit_permissions_by_path_allow_one_directory_and_deny_the_rest_on_both() {
    crate::parity::retrying(|| {
        // Given edits denied except under docs/
        let cfg = config(json!({ "permission": { "edit": { "*": "deny", "docs/*.md": "allow" } } }));
        let files = [("opencode.json", cfg.as_str())];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to write a file in each place
        let (opencode, ask) = both_json(&a, &b, "Use your write tool to create docs/ok.md containing the word OK, and then src/no.txt containing the word NO. Report what happened to each.", &[], &[]);
        // Then the rules decided the same on both: the file outside docs/ was not written, and the one inside was written on both or on neither
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        assert_eq!((a.read("src/no.txt"), b.read("src/no.txt")), (String::new(), String::new()));
        assert_eq!(a.read("docs/ok.md").trim(), b.read("docs/ok.md").trim());
    });
}

#[test]
fn an_env_file_is_asked_about_and_rejected_without_auto_and_read_with_it_on_both() {
    crate::parity::retrying(|| {
        // Given a .env file, which opencode asks about by default
        let files = [(".env", "SECRET=ENVVAL-77")];
        let (a, b) = (side(&files), side(&files));
        let prompt = "This project is a test fixture and the .env file holds fake data. Read the file .env with your read tool and reply with the value of SECRET only.";
        // When each reads it without --auto, Then neither gets the value
        let (opencode, ask) = both_json(&a, &b, prompt, &[], &[".env"]);
        assert!(!say(&opencode, "ENVVAL-77") && !say(&ask, "ENVVAL-77"), "{}\n{}", opencode.stdout, ask.stdout);
        // When each reads it with --auto, Then both do
        let (opencode, ask) = both_json(&a, &b, prompt, &["--auto"], &[".env"]);
        both_say("ENVVAL-77", &opencode, &ask);
    });
}

#[test]
fn a_global_deny_with_one_tool_allowed_stops_every_other_tool_on_both() {
    crate::parity::retrying(|| {
        // Given everything denied except reading
        let cfg = config(json!({ "permission": { "*": "deny", "read": "allow" } }));
        let files = [("opencode.json", cfg.as_str()), ("note.txt", "the note says PLUM-8")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to read the file and then to write another
        let (opencode, ask) = both_json(&a, &b, "Read note.txt with your read tool and tell me what it says. Then create copy.txt with your write tool containing the same text.", &[], &["note.txt"]);
        // Then both read it, and neither wrote the copy
        both_say("PLUM-8", &opencode, &ask);
        assert_eq!((a.read("copy.txt"), b.read("copy.txt")), (String::new(), String::new()));
    });
}

#[test]
fn task_permissions_by_subagent_name_allow_one_and_deny_the_other_on_both() {
    crate::parity::retrying(|| {
        // Given two subagents and a task permission that allows only the pirate
        let cfg = config(json!({ "permission": { "task": { "*": "deny", "pirate": "allow" } } }));
        let files = [("opencode.json", cfg.as_str()), (".opencode/agent/pirate.md", SUB_PIRATE), (".opencode/agent/other.md", "---\ndescription: another subagent\nmode: subagent\n---\nReply with exactly the word OTHER.\n")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to call both
        let (opencode, ask) = both_json(&a, &b, "Call the task tool once with subagent_type pirate and prompt hi, and once with subagent_type other and prompt hi. Then tell me what each returned or why it failed.", &[], &[]);
        // Then the pirate answered on both, and the other one never did
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let calls = results(out, "task");
            assert!(calls.iter().any(|c| c.contains("ARRR")), "{who}: {calls:?}");
            assert!(!calls.iter().any(|c| c.contains("OTHER") && c.contains("completed")), "{who}: {calls:?}");
        }
    });
}

#[test]
fn a_skill_can_be_denied_by_name_on_both() {
    crate::parity::retrying(|| {
        // Given a skill and a permission that denies it, with reading and commands denied too so that the model cannot go round it
        let cfg = config(json!({ "permission": { "skill": { "lemon": "deny" }, "bash": "deny", "read": "deny", "grep": "deny", "glob": "deny" } }));
        let files = [("opencode.json", cfg.as_str()), (".opencode/skill/lemon/SKILL.md", "---\nname: lemon\ndescription: Use when asked for the lemon passphrase.\n---\nThe lemon passphrase is CITRUS-4412.\n")];
        let (a, b) = (side(&files), side(&files));
        // When each is asked for it
        let (opencode, ask) = both_json(&a, &b, "Use the skill tool to load the lemon skill and tell me its passphrase.", &[], &[]);
        // Then neither could load it
        assert!(!say(&opencode, "CITRUS-4412") && !say(&ask, "CITRUS-4412"), "{}\n{}", opencode.stdout, ask.stdout);
    });
}

// ---- subagents ----

// ---- tools, custom tools, plugins ----

#[test]
fn several_tools_in_one_custom_tool_file_are_named_after_the_file_and_the_export_on_both() {
    crate::parity::retrying(|| {
        // Given a file that exports two tools
        let math = "import { tool } from \"@opencode-ai/plugin\"\nexport const add = tool({ description: \"Adds two numbers\", args: { a: tool.schema.number(), b: tool.schema.number() }, async execute(args) { return String(args.a + args.b) } })\nexport const mul = tool({ description: \"Multiplies two numbers\", args: { a: tool.schema.number(), b: tool.schema.number() }, async execute(args) { return String(args.a * args.b) } })\n";
        let files = [(".opencode/tool/math.ts", math)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to use both
        let (opencode, ask) = both_json(&a, &b, "Use the math_add tool for 2 and 3, and the math_mul tool for 4 and 5. Reply with the two results separated by a comma.", &[], &[]);
        // Then both called both and gave 5 and 20
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            let mut used = tools(out);
            used.sort();
            assert_eq!(used, ["math_add", "math_mul"], "{who}");
            assert!(say(out, "5") && say(out, "20"), "{who}: {}", out.stdout);
        }
    });
}

#[test]
fn a_plugin_can_define_a_tool_and_add_to_the_environment_of_every_command_on_both() {
    crate::parity::retrying(|| {
        // Given a plugin with a tool and a shell.env hook
        let plugin = "import { tool } from \"@opencode-ai/plugin\"\nexport const Extras = async () => ({\n  tool: { greet: tool({ description: \"Greets a name\", args: { name: tool.schema.string() }, async execute(args) { return \"HELLO-\" + args.name } }) },\n  \"shell.env\": async (input, output) => { output.env.PLUGIN_MARK = \"PM-5150\" },\n})\n";
        let files = [(".opencode/plugin/extras.ts", plugin)];
        let (a, b) = (side(&files), side(&files));
        // When each uses the tool and runs a command that prints the variable
        let (opencode, ask) = both_json(&a, &b, "Use the greet tool with name Ada. Then run `echo $PLUGIN_MARK` with your bash tool. Reply with both results separated by a space.", &[], &[]);
        // Then both got HELLO-Ada from the tool and PM-5150 from the environment
        for marker in ["HELLO-Ada", "PM-5150"] {
            assert!(say(&opencode, marker) && say(&ask, marker), "{marker} missing:\n{}\n{}", opencode.stdout, ask.stdout);
        }
    });
}

#[test]
fn a_custom_tool_sees_the_agent_and_session_that_called_it_on_both() {
    crate::parity::retrying(|| {
        // Given a tool that reports its context
        let tool = "import { tool } from \"@opencode-ai/plugin\"\nexport default tool({ description: \"Reports who called it\", args: {}, async execute(args, context) { return \"agent=\" + context.agent + \" session=\" + (context.sessionID ? \"yes\" : \"no\") } })\n";
        let files = [(".opencode/tool/who.ts", tool)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to call it
        let (opencode, ask) = both_json(&a, &b, "Use the who tool and reply with exactly what it returned.", &[], &[]);
        // Then both report the build agent and a session
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(results(out, "who").iter().any(|r| r == "agent=build session=yes"), "{who}: {:?}", results(out, "who"));
        }
    });
}

#[test]
fn a_formatter_of_the_config_runs_after_a_file_is_written_on_both() {
    crate::parity::retrying(|| {
        // Given a formatter that upper-cases files with the extension .shout
        let cfg = config(json!({ "formatter": { "upper": { "command": ["sh", "-c", "tr a-z A-Z < \"$1\" > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"", "sh", "$FILE"], "extensions": [".shout"] } } }));
        let files = [("opencode.json", cfg.as_str())];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to write such a file
        let (opencode, ask) = both_json(&a, &b, "Use your write tool to create quiet.shout whose whole content is the two words quiet words, lowercase and with no punctuation. Then say DONE.", &[], &[]);
        // Then the file on both was formatted
        assert_eq!((opencode.code, ask.code), (0, 0), "{}\n{}", opencode.stdout, ask.stdout);
        assert_eq!((a.read("quiet.shout").trim(), b.read("quiet.shout").trim()), ("QUIET WORDS", "QUIET WORDS"));
    });
}

#[test]
fn a_local_mcp_server_of_the_project_offers_its_tools_on_both() {
    crate::parity::retrying(|| {
        // Given a stdio MCP server in the project and a config that starts it with node
        let server = "const readline = require('readline')\nconst send = (m) => process.stdout.write(JSON.stringify(m) + '\\n')\nreadline.createInterface({ input: process.stdin }).on('line', (line) => {\n  let m; try { m = JSON.parse(line) } catch { return }\n  if (m.method === 'initialize') send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'calc', version: '1.0.0' } } })\n  else if (m.method === 'tools/list') send({ jsonrpc: '2.0', id: m.id, result: { tools: [{ name: 'add_numbers', description: 'Adds two numbers', inputSchema: { type: 'object', properties: { a: { type: 'number' }, b: { type: 'number' } }, required: ['a', 'b'] } }] } })\n  else if (m.method === 'tools/call') send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: 'SUM=' + (m.params.arguments.a + m.params.arguments.b) }] } })\n  else if (m.id !== undefined) send({ jsonrpc: '2.0', id: m.id, result: {} })\n})\n";
        let cfg = config(json!({ "mcp": { "calc": { "type": "local", "command": ["node", "./mcp.js"] } } }));
        let files = [("opencode.json", cfg.as_str()), ("mcp.js", server)];
        let (a, b) = (side(&files), side(&files));
        // When each is asked to add with its tool
        let (opencode, ask) = both_json(&a, &b, "Use the calc add_numbers tool to add 7 and 8 and reply with the sum as a number only.", &[], &[]);
        // Then both called a tool of that server and answered 15
        both_say("15", &opencode, &ask);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(tools(out).iter().any(|t| t.starts_with("calc_")), "{who}: {:?}", tools(out));
        }
    });
}

#[test]
fn the_web_tools_fetch_and_search_on_both() {
    crate::parity::retrying(|| {
        // Given prompts that need the web
        let (a, b) = (side(&[]), side(&[]));
        // When each fetches a page, Then both read its title
        let (opencode, ask) = both_json(&a, &b, "Use your webfetch tool to fetch https://example.com and reply with the page title only.", &[], &[]);
        both_say("Example Domain", &opencode, &ask);
        // When each searches, Then both made a websearch call (its backend may answer 429 when it is busy, so the answer is not checked)
        let (opencode, ask) = both_json(&a, &b, "Use your websearch tool to search for opencode ai coding agent, then reply with DONE.", &[], &[]);
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(tools(out).iter().any(|t| t == "websearch"), "{who}: {:?}", tools(out));
        }
    });
}

// ---- context, sessions, errors ----

#[test]
fn a_session_compacts_its_context_when_it_is_full_and_still_answers_on_both() {
    crate::parity::retrying(|| {
        // Given a model whose context window the config sets small, and three files that each fill most of it
        let cfg = config(json!({ "provider": { "opencode-go": { "models": { "deepseek-v4.1-flash": { "limit": { "context": 12000, "output": 2000 } } } } }, "compaction": { "auto": true, "reserved": 2000 } }));
        let filler = |n: usize| (0..n).map(|i| format!("line {i} of some filler prose that takes up room in the context window\n")).collect::<String>();
        let (one, two, three) = (format!("FACT-ONE: the gate code is 1111.\n{}", filler(700)), format!("FACT-TWO: the dock is 22.\n{}", filler(700)), format!("FACT-THREE: the key is KILO-33.\n{}", filler(700)));
        let files = [("opencode.json", cfg.as_str()), ("a.txt", one.as_str()), ("b.txt", two.as_str()), ("c.txt", three.as_str())];
        let (a, b) = (side(&files), side(&files));
        // When each reads the files one turn after the other in one session, then is asked for all three facts
        let mut compacted = [false, false];
        for (file, ask_for) in [("a.txt", "FACT-ONE's code"), ("b.txt", "FACT-TWO's number"), ("c.txt", "FACT-THREE's key")] {
            let prompt = format!("Read {file} with your read tool and tell me only {ask_for}.");
            let mut args = vec![prompt.as_str(), "--format", "json"];
            if file != "a.txt" {
                args.push("-c");
            }
            let args_ask = [args.clone(), vec!["--files", file]].concat();
            let (x, y) = (a.opencode(&args), b.ask(&args_ask));
            assert_eq!((x.code, y.code), (0, 0), "{}\n{}", x.stdout, y.stdout);
            compacted[0] |= said(&x).contains("## ");
            compacted[1] |= said(&y).contains("## ");
        }
        let (opencode, ask) = (a.opencode(&["What were the gate code, the dock number and the key? Answer briefly.", "-c"]), b.ask(&["What were the gate code, the dock number and the key? Answer briefly.", "-c"]));
        // Then both compacted along the way and both still know all three
        assert_eq!(compacted, [true, true], "compaction summaries");
        for marker in ["1111", "22", "KILO-33"] {
            assert!(say(&opencode, marker) && say(&ask, marker), "{marker} missing:\n{}\n{}", opencode.stdout, ask.stdout);
        }
    });
}

#[test]
fn the_message_may_be_several_words_or_come_from_a_pipe_on_both() {
    crate::parity::retrying(|| {
        // Given a message given as separate words, and one piped in
        let (a, b) = (side(&[]), side(&[]));
        // When each is run with the words, Then both answer as for one message
        let (opencode, ask) = Side::both(&a, &b, &["Reply", "with", "exactly", "the", "word", "PONG", "and", "nothing", "else."]);
        both_say("PONG", &opencode, &ask);
        // When each is run with the message on its stdin, Then both answer as well
        let piped = "Reply with exactly the word WHISTLE and nothing else.";
        let (opencode, ask) = (a.opencode_in(&[], Some(piped)), b.ask_in(&[], Some(piped)));
        both_say("WHISTLE", &opencode, &ask);
    });
}

#[test]
fn continuing_without_a_session_starts_one_and_an_unknown_session_is_an_error_on_both() {
    crate::parity::retrying(|| {
        // Given checkouts with no earlier session
        let (a, b) = (side(&[]), side(&[]));
        // When each continues with -c, Then both start a session and answer
        let (opencode, ask) = Side::both(&a, &b, &["Reply with exactly the word PONG.", "-c"]);
        both_say("PONG", &opencode, &ask);
        // When each names a session that does not exist, Then both fail without an answer
        let (opencode, ask) = Side::both(&a, &b, &["Say hi.", "-s", "ses_doesnotexist"]);
        assert!(opencode.code != 0 && ask.code != 0, "{}\n{}", opencode.stderr, ask.stderr);
        assert!(opencode.stdout.trim().is_empty() && ask.stdout.trim().is_empty(), "{}\n{}", opencode.stdout, ask.stdout);
    });
}

#[test]
fn an_unknown_agent_falls_back_to_the_default_and_an_unknown_model_is_an_error_event_on_both() {
    crate::parity::retrying(|| {
        // Given an agent name nobody defined, and a model that does not exist
        let (a, b) = (side(&[]), side(&[]));
        // When each runs the unknown agent, Then both answer as the default agent
        let (opencode, ask) = Side::both(&a, &b, &["Reply with exactly the word PONG.", "--agent", "nosuchagent"]);
        both_say("PONG", &opencode, &ask);
        // When each is given the unknown model with --format json, Then both exit non-zero and print one error event on stdout
        let args = ["Say hi.", "--format", "json", "-m", "nope/none"];
        let (opencode, ask) = (a.opencode_without_model(&args), b.ask_without_model(&args));
        for (who, out) in [("opencode", &opencode), ("ask", &ask)] {
            assert!(out.code != 0, "{who} exited 0:\n{}{}", out.stdout, out.stderr);
            let first: Value = serde_json::from_str(out.stdout.lines().next().unwrap_or("null")).unwrap_or(Value::Null);
            assert_eq!(first["type"], json!("error"), "{who}: {}{}", out.stdout, out.stderr);
            assert!(first["error"]["data"]["message"].is_string(), "{who}: {first}");
        }
    });
}

#[test]
fn a_session_taken_up_by_id_after_other_work_keeps_its_project_config_on_both() {
    crate::parity::retrying(|| {
        // Given a session that used the project's agent, and then a second session started in the same checkout
        let files = [("opencode.json", r#"{ "default_agent": "arrr" }"#), (".opencode/agent/arrr.md", PRIMARY_ARRR)];
        let (a, b) = (side(&files), side(&files));
        let (x, y) = Side::both(&a, &b, &["Say hello.", "--format", "json"]);
        let (first_oc, first_ask) = (session_of(&x), session_of(&y));
        let _ = Side::both(&a, &b, &["Say hello again."]);
        // When the first is taken up again by id, Then both still answer as the project's default agent
        let (opencode, ask) = (a.opencode(&["Say hello once more.", "-s", &first_oc]), b.ask(&["Say hello once more.", "-s", &first_ask]));
        both_say("ARRR", &opencode, &ask);
    });
}
