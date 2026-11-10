//! `agent.catalog` end to end: the agent CLIs and skills are the EXECUTOR's, found by the real `rca-exec` on its own `PATH` and in its own home
//! and never on the service's, and `models.list` is the service's table. Every recorded exchange of the contract is replayed.
use crate::harness::{service, Env, Options, Service, SECRET};
use crate::mapper::Agent;
use exec_testkit::script::{Script, Turn};
use exec_testkit::{RcaExec, ScriptedCli, TempRepo};
use serde_json::{json, Value};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

/// The service, a repository served as `w1` by the real executor, and the executor's `PATH`: the scripted `claude` and `codex` with the versions
/// the recorded exchanges name, no `opencode`.
struct Rig {
    _svc: Service,
    _cli: ScriptedCli,
    _exec: RcaExec,
    _path: tempfile::TempDir,
    repo: TempRepo,
    token: String,
    port: u16,
}

async fn rig(env: &Env) -> Rig {
    let cli = ScriptedCli::install();
    let path = tempfile::tempdir().unwrap();
    for (program, version) in agent_api_catalog::testing::FIXTURE_CLIS {
        cli.script(program, &Script::of(vec![Turn::default().then_line("a turn")]).printing_version(version));
        symlink(cli.dir().join(program), path.path().join(program)).unwrap();
    }
    let repo = TempRepo::new();
    let svc = service(env, Options::default());
    let port = svc.listen().await;
    let executor_path = std::env::join_paths([path.path(), std::path::Path::new("/usr/bin"), std::path::Path::new("/bin")]).unwrap();
    let exec = tokio::task::block_in_place(|| RcaExec::serve_on(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET, &[], &executor_path));
    let token = svc.mint("flow mapper").await;
    Rig { _svc: svc, _cli: cli, _exec: exec, _path: path, repo, token, port }
}

impl Rig {
    async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }

    fn executor_home(&self) -> PathBuf {
        self.repo.path().parent().unwrap().join("home")
    }
}

fn skill(base: &std::path::Path, dir: &str, name: &str, description: &str) {
    let at = base.join(dir).join(name);
    std::fs::create_dir_all(&at).unwrap();
    std::fs::write(at.join("SKILL.md"), format!("---\nname: {name}\ndescription: {description}\n---\nBody\n")).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_skills_on_the_executor_and_others_on_the_service_when_listed_then_only_the_executors_come_back_and_its_home_is_named_below_a_tilde() {
    // Given the executor's repository and home holding skills, and the service's own home and working directory holding others
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    skill(&rig.repo.path(), ".claude/skills", "review", "Reviews a diff");
    skill(&rig.executor_home(), ".claude/skills", "mine", "A personal skill");
    skill(&env.home, ".claude/skills", "on-the-service", "Never listed");
    let mut agent = rig.agent().await;
    // When the skills of the workspace are listed, and the commands claude and codex read
    let listed = agent.ask("skills.list", json!({"workspace": "w1"})).await;
    let claude = agent.ask("commands.list", json!({"driver": "claudeAgent", "workspace": "w1"})).await;
    let codex = agent.ask("commands.list", json!({"driver": "codex", "workspace": "w1"})).await;
    // Then the workspace's skill comes first and the executor's home's second, by a path that does not say where the home is
    let skills: Vec<(&str, &str, &str)> = listed["result"]["skills"].as_array().unwrap().iter().map(|s| (s["name"].as_str().unwrap(), s["scope"].as_str().unwrap(), s["path"].as_str().unwrap())).collect();
    assert_eq!(skills, [("review", "workspace", ".claude/skills/review/SKILL.md"), ("mine", "home", "~/.claude/skills/mine/SKILL.md")], "{listed}");
    assert!(!listed.to_string().contains(rig.executor_home().to_str().unwrap()) && !listed.to_string().contains("on-the-service"), "{listed}");
    // And claude reads those two, codex reads neither directory
    let names = |answer: &Value| answer["result"]["commands"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!((names(&claude), names(&codex)), (vec!["review".to_string(), "mine".to_string()], Vec::<String>::new()), "{claude} {codex}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_the_models_of_a_driver_when_listed_then_the_default_the_turn_takes_is_among_them() {
    // Given the service
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    for driver in agent_api_catalog::DRIVERS {
        // When the models of each driver are listed
        let answer = agent.ask("models.list", json!({"driver": driver})).await;
        // Then exactly one is the default
        let models = answer["result"]["models"].as_array().unwrap_or_else(|| panic!("{answer}"));
        assert_eq!(models.iter().filter(|m| m["is_default"] == json!(true)).count(), 1, "{driver}: {answer}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_the_catalog_contract_has_recorded_exchanges_that_the_service_answers_as_recorded_over_the_real_executor() {
    // Given the recorded exchanges of the catalog crate and its methods
    let recorded = agent_api_catalog::testing::fixtures();
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    let mut methods: Vec<&str> = agent_api_catalog::METHODS.iter().map(|m| m.name()).collect();
    methods.sort_unstable();
    assert_eq!(named, methods);
    for fixture in &recorded {
        for exchange in &fixture.exchanges {
            // When an exchange's setup and request are made on a service and executor of their own
            let env = Env::new(&[]);
            let rig = rig(&env).await;
            let mut agent = rig.agent().await;
            for before in &exchange.before {
                let answered = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
                assert!(answered.get("result").is_some(), "{}: the setup {before} failed: {answered}", exchange.name);
            }
            let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            answer["id"] = exchange.response["id"].clone();
            // Then the answer is the recorded one, and the executor actions it recorded are ones the table declares for the method
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
            let declared: Vec<&str> = rca_group_catalog::SERVED.iter().find(|s| s.method.name() == fixture.method).map(|s| s.actions.to_vec()).unwrap();
            assert!(exchange.actions.iter().all(|a| declared.contains(&a.as_str())), "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
        }
    }
}
