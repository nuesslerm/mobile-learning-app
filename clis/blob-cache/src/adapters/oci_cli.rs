//! `BlobAdmin` over the `oci` CLI and the operator's key in `~/.oci`: usage, listing and deletion, which no pre-authenticated
//! request can do. The data plane never uses it and nothing that runs on every push depends on it.
use crate::ports::{BlobAdmin, ObjectInfo, StoreError, Usage};
use crate::storage_guard::{sanitize, BucketSize, Deleted, Doomed, Tenancy};
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// `${name_prefix}-build-cache` of `infra/oracle-cloud/build-cache.tf`.
pub const BUCKET: &str = "mobile-learning-build-cache";
const DELETE_WORKERS: usize = 8;
const STOPPED: &str = "stopped at the time limit";

pub struct OciCli {
    bucket: String,
    namespace: OnceLock<String>,
}

impl OciCli {
    pub fn new(bucket: &str) -> OciCli {
        OciCli { bucket: bucket.to_string(), namespace: OnceLock::new() }
    }

    fn run(&self, args: &[&str]) -> Result<String, StoreError> {
        let verb = args.iter().take(3).copied().collect::<Vec<_>>().join(" ");
        let out = Command::new("oci")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| StoreError::Unreachable(format!("oci {verb}: {error} (the admin verbs need the oci CLI and ~/.oci)")))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let said: String = sanitize(&String::from_utf8_lossy(&out.stderr)).chars().take(300).collect();
        Err(StoreError::Unreachable(format!("oci {verb}: {said}")))
    }

    /// `run` that kills the CLI when `deadline` passes.
    fn run_until(&self, args: &[&str], deadline: Instant) -> Result<String, StoreError> {
        let verb = args.iter().take(3).copied().collect::<Vec<_>>().join(" ");
        let mut child = Command::new("oci")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| StoreError::Unreachable(format!("oci {verb}: {error} (the admin verbs need the oci CLI and ~/.oci)")))?;
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                bytes
            })
        };
        let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(StoreError::Unreachable(format!("oci {verb}: {STOPPED}")));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => return Err(StoreError::Unreachable(format!("oci {verb}: {error}"))),
            }
        };
        let (stdout, stderr) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        if status.success() {
            return Ok(String::from_utf8_lossy(&stdout).into_owned());
        }
        Err(StoreError::Unreachable(format!("oci {verb}: {}", sanitize(&String::from_utf8_lossy(&stderr)).chars().take(300).collect::<String>())))
    }

    fn json(&self, args: &[&str]) -> Result<Value, StoreError> {
        self.parse(args, self.run(args)?)
    }

    fn json_until(&self, args: &[&str], deadline: Instant) -> Result<Value, StoreError> {
        self.parse(args, self.run_until(args, deadline)?)
    }

    fn parse(&self, args: &[&str], text: String) -> Result<Value, StoreError> {
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|error| StoreError::Rejected(format!("oci {}: unparseable output: {error}", args.iter().take(3).copied().collect::<Vec<_>>().join(" "))))
    }

    fn namespace(&self) -> Result<&str, StoreError> {
        if let Some(found) = self.namespace.get() {
            return Ok(found);
        }
        let named = self.json(&["os", "ns", "get"])?["data"].as_str().filter(|text| !text.is_empty()).map(str::to_string);
        let named = named.ok_or_else(|| StoreError::Rejected("oci os ns get: no namespace in the answer".into()))?;
        Ok(self.namespace.get_or_init(|| named))
    }

    fn requests_this_month(&self, compartment: &str) -> Result<u64, StoreError> {
        if !self.bucket.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
            return Err(StoreError::Rejected(format!("bucket name {:?} cannot go into a metrics query", self.bucket)));
        }
        let now = now_ms();
        let start = month_start_ms(now);
        let query = format!("AllRequests[1d]{{resourceDisplayName = \"{}\"}}.sum()", self.bucket);
        let series = self.json(&[
            "monitoring",
            "metric-data",
            "summarize-metrics-data",
            "--namespace",
            "oci_objectstorage",
            "--compartment-id",
            compartment,
            "--query-text",
            &query,
            "--start-time",
            &utc_text(start),
            "--end-time",
            &utc_text(now.max(start + 60_000)),
        ])?;
        let total: f64 = series["data"].as_array().into_iter().flatten().flat_map(|one| one["aggregated-datapoints"].as_array().into_iter().flatten()).filter_map(|point| point["value"].as_f64()).sum();
        Ok(total.round() as u64)
    }

    fn delete_one(&self, namespace: &str, item: &Doomed, deadline: Option<Instant>) -> Result<Gone, StoreError> {
        let mut args = vec!["os", "object", "delete", "--namespace-name", namespace, "--bucket-name", &self.bucket, "--object-name", &item.name, "--force"];
        if let Some(etag) = &item.etag {
            args.extend(["--if-match", etag]);
        }
        let ran = match deadline {
            Some(deadline) => self.run_until(&args, deadline),
            None => self.run(&args),
        };
        match ran {
            Ok(_) => Ok(Gone::Deleted),
            // the lifecycle rule or another gc got there first
            Err(error) if error.to_string().contains("ObjectNotFound") => Ok(Gone::Missing),
            // rewritten after it was listed: it is a fresh object now
            Err(error) if ["IfMatchFailed", "PreconditionFailed", "\"status\": 412"].iter().any(|m| error.to_string().contains(m)) => Ok(Gone::Changed),
            Err(error) => Err(error),
        }
    }

    /// Deletes with `DELETE_WORKERS` at a time, each stopping at its first failure or at `deadline`; what was deleted is reported even then.
    pub fn delete_items(&self, items: &[Doomed], deadline: Option<Instant>) -> Deleted {
        let mut total = Deleted::default();
        if items.is_empty() {
            return total;
        }
        let namespace = match self.namespace() {
            Ok(namespace) => namespace,
            Err(error) => {
                total.error = Some(error);
                return total;
            }
        };
        let per_worker = items.len().div_ceil(DELETE_WORKERS);
        let parts: Vec<Deleted> = std::thread::scope(|scope| {
            let workers: Vec<_> = items
                .chunks(per_worker)
                .map(|part| {
                    scope.spawn(move || {
                        let mut got = Deleted::default();
                        for item in part {
                            if deadline.is_some_and(|d| Instant::now() >= d) {
                                break;
                            }
                            match self.delete_one(namespace, item, deadline) {
                                Ok(Gone::Deleted) => got.done.push(item.name.clone()),
                                Ok(Gone::Missing) => got.missing += 1,
                                Ok(Gone::Changed) => got.changed += 1,
                                Err(error) if error.to_string().contains(STOPPED) => break,
                                Err(error) => {
                                    got.error = Some(error);
                                    break;
                                }
                            }
                        }
                        got
                    })
                })
                .collect();
            workers.into_iter().map(|worker| worker.join().unwrap_or_else(|_| Deleted { error: Some(StoreError::Unreachable("a delete worker panicked".into())), ..Deleted::default() })).collect()
        });
        for part in parts {
            total.done.extend(part.done);
            total.missing += part.missing;
            total.changed += part.changed;
            total.error = total.error.or(part.error);
        }
        total
    }
}

enum Gone {
    Deleted,
    Missing,
    Changed,
}

fn count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
}

impl BlobAdmin for OciCli {
    fn usage(&self) -> Result<Usage, StoreError> {
        let namespace = self.namespace()?;
        let bucket = self.json(&["os", "bucket", "get", "--namespace-name", namespace, "--bucket-name", &self.bucket, "--fields", "approximateSize", "--fields", "approximateCount"])?;
        let data = &bucket["data"];
        let field = |name: &str| count(&data[name]).ok_or_else(|| StoreError::Rejected(format!("oci os bucket get: no {name} in the answer")));
        let compartment = data["compartment-id"].as_str().ok_or_else(|| StoreError::Rejected("oci os bucket get: no compartment-id in the answer".into()))?;
        Ok(Usage { stored_bytes: field("approximate-size")?, objects: field("approximate-count")?, requests_this_month: Some(self.requests_this_month(compartment)?) })
    }

    fn list(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StoreError> {
        self.list_until(prefix, None)
    }

    fn delete(&self, names: &[String]) -> Result<usize, StoreError> {
        let items: Vec<Doomed> = names.iter().map(|name| Doomed { name: name.clone(), etag: None }).collect();
        let got = self.delete_items(&items, None);
        match got.error {
            Some(error) => Err(error),
            None => Ok(got.done.len() + got.missing),
        }
    }
}

impl OciCli {
    fn list_until(&self, prefix: &str, deadline: Option<Instant>) -> Result<Vec<ObjectInfo>, StoreError> {
        let namespace = self.namespace()?;
        let mut args = vec!["os", "object", "list", "--namespace-name", namespace, "--bucket-name", &self.bucket, "--fields", "name,size,timeCreated,etag", "--all"];
        if !prefix.is_empty() {
            args.extend(["--prefix", prefix]);
        }
        let listed = match deadline {
            Some(deadline) => self.json_until(&args, deadline)?,
            None => self.json(&args)?,
        };
        Ok(listed["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|object| {
                let name = object["name"].as_str()?.to_string();
                let created_ms = object["time-created"].as_str().and_then(parse_utc_ms).unwrap_or(i64::MAX);
                Some(ObjectInfo { name, bytes: count(&object["size"]).unwrap_or(0), created_ms, etag: object["etag"].as_str().map(str::to_string) })
            })
            .collect())
    }

    fn read_until(&self, name: &str, deadline: Instant) -> Result<Option<String>, StoreError> {
        let namespace = self.namespace()?;
        match self.run_until(&["os", "object", "get", "--namespace-name", namespace, "--bucket-name", &self.bucket, "--name", name, "--file", "-"], deadline) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.to_string().contains("ObjectNotFound") => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as i64)
}

// Days since 1970-01-01 and the proleptic Gregorian date, after Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

fn month_start_ms(now: i64) -> i64 {
    let (year, month, _) = civil_from_days(now.div_euclid(86_400_000));
    days_from_civil(year, month, 1) * 86_400_000
}

fn utc_text(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let clock = seconds.rem_euclid(86_400);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", clock / 3600, clock % 3600 / 60, clock % 60)
}

/// `2026-08-27T10:37:58.582000+00:00`, as the oci CLI prints a creation time; a `Z` or any other offset is read too.
fn parse_utc_ms(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut date = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let zone_at = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(zone_at);
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut clock = whole.split(':').map(|part| part.parse::<i64>().ok());
    let (hours, minutes, seconds) = (clock.next()??, clock.next()??, clock.next()??);
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = format!("{fraction:0<3}")[..3].parse().ok()?;
    let offset_minutes = match zone {
        "" | "Z" => 0,
        zone => {
            let (hh, mm) = zone[1..].split_once(':')?;
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            sign * (hh.parse::<i64>().ok()? * 60 + mm.parse::<i64>().ok()?)
        }
    };
    Some((days_from_civil(year, month, day) * 86_400 + hours * 3600 + minutes * 60 + seconds - offset_minutes * 60) * 1000 + millis)
}

/// Every bucket of the tenancy through the `oci` CLI: the compartment is found from the build-cache bucket, which terraform
/// creates beside the others, so no tenancy id is read from `~/.oci`.
pub struct OciTenancy {
    anchor: OciCli,
}

impl Default for OciTenancy {
    fn default() -> OciTenancy {
        OciTenancy { anchor: OciCli::new(BUCKET) }
    }
}

impl OciTenancy {
    fn size(&self, namespace: &str, bucket: &str) -> Result<(u64, Option<String>), StoreError> {
        let got = self.anchor.json(&["os", "bucket", "get", "--namespace-name", namespace, "--bucket-name", bucket, "--fields", "approximateSize"])?;
        let bytes = count(&got["data"]["approximate-size"]).ok_or_else(|| StoreError::Rejected(format!("oci os bucket get {bucket}: no approximate-size in the answer")))?;
        Ok((bytes, got["data"]["compartment-id"].as_str().map(str::to_string)))
    }
}

impl Tenancy for OciTenancy {
    fn buckets(&self) -> Result<Vec<BucketSize>, StoreError> {
        let namespace = self.anchor.namespace()?;
        let (anchor_bytes, compartment) = self.size(namespace, BUCKET)?;
        let compartment = compartment.ok_or_else(|| StoreError::Rejected("oci os bucket get: no compartment-id in the answer".into()))?;
        let listed = self.anchor.json(&["os", "bucket", "list", "--namespace-name", namespace, "--compartment-id", &compartment, "--all"])?;
        let mut sizes = Vec::new();
        for name in listed["data"].as_array().into_iter().flatten().filter_map(|b| b["name"].as_str()) {
            let bytes = if name == BUCKET { anchor_bytes } else { self.size(namespace, name)?.0 };
            sizes.push(BucketSize { name: name.to_string(), bytes });
        }
        if !sizes.iter().any(|b| b.name == BUCKET) {
            sizes.push(BucketSize { name: BUCKET.to_string(), bytes: anchor_bytes });
        }
        Ok(sizes)
    }

    fn list(&self, bucket: &str, prefix: &str, deadline: Instant) -> Result<Vec<ObjectInfo>, StoreError> {
        OciCli::new(bucket).list_until(prefix, Some(deadline))
    }

    fn read(&self, bucket: &str, name: &str, deadline: Instant) -> Result<Option<String>, StoreError> {
        OciCli::new(bucket).read_until(name, deadline)
    }

    fn delete(&self, bucket: &str, items: &[Doomed], deadline: Instant) -> Deleted {
        OciCli::new(bucket).delete_items(items, Some(deadline))
    }
}
