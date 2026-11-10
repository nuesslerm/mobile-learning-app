//! The toy reference of the flow tests: a `mimic-driver/1` server over stdin and stdout whose output a recorder must mask. `echo` returns
//! its input, `session` makes up an id, a timestamp, a token and a sequence on every run, `reject` is an error outcome, `wobble`
//! returns a number no rule masks, `counter` counts the requests since the last `reset` (state a recording must keep apart). With
//! `MIMIC_TOY_DELAY_MS=<n>` makes every request take n ms (a slow reference, for the `--max-minutes` flow tests). `MIMIC_TOY_DIE_AFTER=<n>` the program exits after answering n requests, the planted kill of the resume flow tests. `--tweak change-value|drop-key|extra-key` plants a behaviour change, for a candidate.
use mimic_core::cassette::{Dir, Outcome, Status, Step, StepKind};
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request, Rng};
use mimic_core::testing::{ToyDriver, Tweak};
use serde_json::json;
use wf_mimic::adapters::{entropy, serve, SystemClock};

struct Toy<R: Rng> {
    inner: ToyDriver<R, SystemClock>,
    counter: u64,
    served: u64,
    delay_ms: u64,
    die_after: Option<u64>,
}

impl<R: Rng> Driver for Toy<R> {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        self.inner.hello()
    }

    fn reset(&mut self) -> Result<(), DriverError> {
        self.counter = 0;
        Ok(())
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        if self.die_after == Some(self.served) {
            std::process::exit(9);
        }
        self.served += 1;
        std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        let payload = match request.op.as_deref() {
            Some("wobble") => json!({"n": self.inner.rng.next_u64()}),
            Some("counter") => {
                self.counter += 1;
                json!({"n": self.counter})
            }
            _ => return self.inner.run(request),
        };
        let step = Step { dir: Dir::Ret, channel: None, kind: StepKind::Json, rid: None, len: None, payload };
        Ok(Observed { steps: vec![step], effects: Vec::new(), outcome: Outcome { status: Status::Ok, code: None, reference_accepts: Some(true) } })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tweak = match args.iter().position(|a| a == "--tweak").and_then(|i| args.get(i + 1)).map(String::as_str) {
        None | Some("none") => Tweak::None,
        Some("change-value") => Tweak::ChangeValue,
        Some("drop-key") => Tweak::DropKey,
        Some("extra-key") => Tweak::ExtraKey,
        Some(other) => {
            eprintln!("mimic-toy: unknown tweak {other} (none, change-value, drop-key, extra-key)");
            std::process::exit(2);
        }
    };
    let die_after = std::env::var("MIMIC_TOY_DIE_AFTER").ok().and_then(|v| v.parse().ok());
    let mut toy = Toy { inner: ToyDriver { rng: entropy(), clock: SystemClock, tweak }, counter: 0, served: 0, delay_ms: std::env::var("MIMIC_TOY_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0), die_after };
    if let Err(e) = serve(&mut toy, std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("mimic-toy: {e}");
        std::process::exit(1);
    }
}
