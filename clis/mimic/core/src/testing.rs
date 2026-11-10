//! In-memory doubles of every port, for the flow tests of this crate and of the thin crate: `MemStore`, `ScriptedDriver`, `ToyDriver`,
//! `FixedClock`, `FakeWire`.
use crate::cassette::{Dir, DriverInfo, Effect, Outcome, Status, Step, StepKind};
use crate::ports::{CassetteSet, Clock, Driver, DriverError, Hello, Observed, Request, Rng, Store, StoreError};
use crate::rng::SplitMix64;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct MemStore {
    pub sets: BTreeMap<String, CassetteSet>,
    pub files: BTreeMap<(String, String), Value>,
    pub raw: BTreeMap<(String, String), Vec<u8>>,
}

impl Store for MemStore {
    fn list(&self) -> Result<Vec<String>, StoreError> {
        Ok(self.sets.keys().cloned().collect())
    }
    fn read(&self, set: &str) -> Result<CassetteSet, StoreError> {
        self.sets.get(set).cloned().ok_or_else(|| StoreError::NotFound(set.into()))
    }
    fn write(&mut self, set: &str, content: &CassetteSet) -> Result<(), StoreError> {
        self.sets.insert(set.into(), content.clone());
        Ok(())
    }
    fn read_corpus_file(&self, set: &str, rel: &str) -> Result<Value, StoreError> {
        self.files.get(&(set.into(), rel.into())).cloned().ok_or_else(|| StoreError::NotFound(format!("{set}/{rel}")))
    }
    fn write_raw(&mut self, set: &str, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.raw.insert((set.into(), name.into()), bytes.to_vec());
        Ok(())
    }
}

pub fn driver_info(name: &str) -> DriverInfo {
    DriverInfo { name: name.into(), version: "1".into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None }
}

/// Canned replies by interaction id; remembers what it was asked.
#[derive(Default)]
pub struct ScriptedDriver {
    pub replies: BTreeMap<String, Result<Observed, String>>,
    pub asked: Vec<Request>,
    pub target_version: Option<String>,
}

impl Driver for ScriptedDriver {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: driver_info("scripted"), target_version: self.target_version.clone(), coverage: false })
    }
    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        self.asked.push(request.clone());
        match self.replies.get(&request.id) {
            Some(Ok(o)) => Ok(o.clone()),
            Some(Err(e)) => Err(DriverError(e.clone())),
            None => Err(DriverError(format!("no scripted reply for {}", request.id))),
        }
    }
}

pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// A planted behaviour change of the toy, to prove replay catches it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tweak {
    None,
    ChangeValue,
    DropKey,
    ExtraKey,
}

/// A toy reference with the three kinds of output mimic normalises: `echo` is deterministic, `session` makes up an id and a timestamp
/// from the injected rng and clock, `reject` is an error outcome. Fresh randomness per run unless the rng is seeded identically.
pub struct ToyDriver<R: Rng, C: Clock> {
    pub rng: R,
    pub clock: C,
    pub tweak: Tweak,
}

impl ToyDriver<SplitMix64, FixedClock> {
    pub fn new(seed: u64, now_ms: u64) -> Self {
        Self { rng: SplitMix64::new(seed), clock: FixedClock(now_ms), tweak: Tweak::None }
    }
}

fn uuid(rng: &mut dyn Rng) -> String {
    let (a, b) = (rng.next_u64(), rng.next_u64());
    format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, (b >> 48) & 0xfff, b & 0xffff_ffff_ffff)
}

fn iso(ms: u64) -> String {
    let (days, rem) = (ms / 86_400_000, ms % 86_400_000);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

impl<R: Rng, C: Clock> Driver for ToyDriver<R, C> {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: driver_info("toy"), target_version: Some("toy-1.0".into()), coverage: false })
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        let op = request.op.as_deref().unwrap_or("echo");
        let mut payload = match op {
            "echo" => request.input.clone(),
            "session" => json!({
                "sessionId": uuid(&mut self.rng),
                "startedAt": iso(self.clock.now_ms()),
                "token": format!("{}.{}", uuid(&mut self.rng).replace('-', ""), uuid(&mut self.rng).replace('-', "")),
                "sequence": self.rng.next_u64() % 1000,
                "user": request.input.get("user").cloned().unwrap_or(Value::Null),
            }),
            "reject" => {
                return Ok(Observed {
                    steps: Vec::new(),
                    effects: Vec::new(),
                    outcome: Outcome { status: Status::Error, code: Some("rejected".into()), reference_accepts: Some(false) },
                })
            }
            other => return Err(DriverError(format!("toy has no op {other}"))),
        };
        if let Value::Object(m) = &mut payload {
            match self.tweak {
                Tweak::None => {}
                Tweak::ChangeValue => {
                    if let Some((_, v)) = m.iter_mut().find(|(_, v)| v.is_string()) {
                        *v = json!("changed");
                    }
                }
                Tweak::DropKey => {
                    if let Some(k) = m.keys().next().cloned() {
                        m.remove(&k);
                    }
                }
                Tweak::ExtraKey => {
                    m.insert("extra".into(), json!(true));
                }
            }
        }
        let mut data = Map::new();
        data.insert("path".into(), json!("/toy/out"));
        Ok(Observed {
            steps: vec![Step { dir: Dir::Ret, channel: None, kind: StepKind::Json, rid: None, len: None, payload }],
            effects: vec![Effect { kind: "fs.write".into(), data }],
            outcome: Outcome { status: Status::Ok, code: None, reference_accepts: Some(true) },
        })
    }
}

/// A tap that has seen what it was told: for tests of what a driver makes of the events.
#[derive(Default)]
pub struct FakeWire {
    pub url: String,
    pub log: Vec<crate::wire::WireEvent>,
}

impl crate::wire::Wire for FakeWire {
    fn url(&self) -> String {
        self.url.clone()
    }
    fn mark(&self) -> usize {
        self.log.len()
    }
    fn since(&self, mark: usize) -> Vec<crate::wire::WireEvent> {
        self.log.iter().skip(mark).cloned().collect()
    }
}

/// A crawler that answers a fixed report (or failure): the double of the `Crawler` port.
pub struct ScriptedCrawler(pub Result<crate::crawl::Report, crate::crawl::CrawlError>);

impl crate::crawl::Crawler for ScriptedCrawler {
    fn crawl(&mut self) -> Result<crate::crawl::Report, crate::crawl::CrawlError> {
        self.0.clone()
    }
}
