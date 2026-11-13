//! `wf lib <verb>`: parse the line, wire the adapters, run the use case, print one JSON object on stdout. A failure prints
//! `{ok: false, error, next}` and exits 1; a malformed line exits 2. No verb prompts and none prints a link.
use crate::adapters::git::{GitRef, WorkingTree};
use crate::domain::libraries::{parse, Catalog};
use crate::domain::version::Bump;
use crate::ports::Failure;
use crate::usecases::{prune, release, retarget, status};
use crate::wiring;
use crate::wiring::LIBS_BUCKET;
use blob_cache::adapters::OciCli;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

const RELEASE: Spec = Spec { positional: Positional::Exactly(2), ..Spec::NONE };
const STATUS: Spec = Spec { positional: Positional::None, ..Spec::NONE };
const PRUNE: Spec = Spec { values: &["grace-days"], flags: &["dry-run", "no-fetch"], positional: Positional::None, ..Spec::NONE };
const RETARGET: Spec = Spec { positional: Positional::Exactly(2), ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "release" => release_verb(&parse_spec(rest, &RELEASE)?),
        "status" => status_verb(&parse_spec(rest, &STATUS)?),
        "prune" => prune_verb(&parse_spec(rest, &PRUNE)?),
        "retarget" => retarget_verb(&parse_spec(rest, &RETARGET)?),
        _ => None,
    }
}

fn finish(verb: &str, outcome: Result<Value, Failure>) -> Option<i32> {
    match outcome {
        Ok(value) => {
            println!("{value}");
            Some(i32::from(value["ok"] == false))
        }
        Err(Failure { error, next }) => {
            wf_kit::wf_err!("wf lib {verb}: {error}");
            println!("{}", json!({ "ok": false, "error": error, "next": next }));
            Some(1)
        }
    }
}

fn catalog(root: &std::path::Path) -> Result<Catalog, Failure> {
    let text = std::fs::read_to_string(root.join("libraries.json")).map_err(|e| Failure::new(format!("libraries.json: {e}")).next("run from inside the repository"))?;
    Ok(parse(&text)?)
}

fn release_verb(p: &Parsed) -> Option<i32> {
    let Some(how) = Bump::parse(&p.positional[1]) else {
        wf_kit::wf_err!("wf lib release: the bump is major, minor or patch, got {:?}", p.positional[1]);
        return Some(2);
    };
    let root = wf_kit::root::repo_root();
    let outcome = catalog(&root).and_then(|catalog| {
        let library = catalog.library(&p.positional[0]).ok_or_else(|| Failure::new(format!("{} is not in libraries.json", p.positional[0])))?;
        let registry = catalog.registry(&library.registry).ok_or_else(|| Failure::new("the library's registry is not in libraries.json"))?;
        let publisher = wiring::publisher(registry)?;
        release::release(&root, &catalog, &p.positional[0], how, publisher.as_ref(), &crate::adapters::guard::GitGuard(root.clone()))
    });
    finish("release", outcome)
}

fn status_verb(_: &Parsed) -> Option<i32> {
    let root = wf_kit::root::repo_root();
    let outcome = catalog(&root).and_then(|catalog| {
        let store = wiring::first_bucket(&catalog).and_then(|r| wiring::index_source(r).ok());
        status::status(&catalog, &WorkingTree(root.clone()), store.as_deref())
    });
    finish("status", outcome)
}

fn prune_verb(p: &Parsed) -> Option<i32> {
    let grace_days = match p.val("grace-days").map(str::parse::<i64>) {
        None => 30,
        Some(Ok(days)) if days >= 0 => days,
        Some(_) => {
            wf_kit::wf_err!("wf lib prune: --grace-days takes a number of days");
            return Some(2);
        }
    };
    let root = wf_kit::root::repo_root();
    let outcome = catalog(&root).and_then(|catalog| {
        if !p.flag("no-fetch") {
            let fetched = Command::new("git").arg("-C").arg(&root).args(["fetch", "--quiet", "origin", "dev"]).stdin(Stdio::null()).output();
            if !fetched.as_ref().is_ok_and(|o| o.status.success()) {
                return Err(Failure::new("git fetch origin dev failed: a stale origin/dev could hide a pin").next("fix the remote, or pass --no-fetch when origin/dev is known to be current"));
            }
        }
        let registry = wiring::first_bucket(&catalog).ok_or_else(|| Failure::new("libraries.json has no registry with a deletable (bucket-sparse) publisher"))?;
        let store = wiring::libs_store(registry, true)?;
        let dev = GitRef { root: root.clone(), rev: "origin/dev".into() };
        let options = prune::Options { dry_run: p.flag("dry-run"), grace_days, now_ms: wf_kit::time::now_ms() };
        prune::prune(&catalog, &dev, store.as_ref(), &OciCli::new(LIBS_BUCKET), &wiring::publisher_can_delete, &options)
    });
    finish("prune", outcome)
}

fn retarget_verb(p: &Parsed) -> Option<i32> {
    let root: PathBuf = wf_kit::root::repo_root();
    let outcome = catalog(&root).and_then(|catalog| retarget::retarget_library(&root, &catalog, &WorkingTree(root.clone()), &p.positional[0], &p.positional[1]));
    finish("retarget", outcome)
}
