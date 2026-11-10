//! The metrics in the Always Free Autonomous Database, through ORDS's SQL endpoint (`<ords root>/<schema alias>/_/sql`) as the
//! `HEALTH_RPT` user. One table, `HOST_SAMPLES`; nothing is rolled up in advance, the longer windows are averaged by the query.
use async_trait::async_trait;
use reporting_core::application::{MetricsStore, PortError};
use reporting_core::domain::sample::{Sample, SeriesPoint, SeriesQuery};
use reporting_core::domain::window::granularity_for;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Rows ORDS returns for one statement unless told otherwise (and the page the reads ask for).
const PAGE: usize = 10_000;
/// Samples written in one statement.
const CHUNK: usize = 400;
/// Samples kept while the database cannot be reached (about five hours of one sweep a minute); the oldest are dropped beyond it.
const BUFFER_CAP: usize = 20_000;
/// `/health` reports the row count this old rather than counting a table of millions of rows every 30 s.
const COUNT_TTL: Duration = Duration::from_secs(600);

pub const SCHEMA: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS HOST_SAMPLES (
       TS NUMBER(19) NOT NULL, SCOPE VARCHAR2(40) NOT NULL, NAME VARCHAR2(200) NOT NULL, METRIC VARCHAR2(80) NOT NULL, VALUE BINARY_DOUBLE NOT NULL,
       CONSTRAINT HOST_SAMPLES_PK PRIMARY KEY (TS, SCOPE, NAME, METRIC)
     )",
    "ALTER TABLE HOST_SAMPLES ENABLE ROW MOVEMENT",
];

/// The SQL endpoint of the schema an auto-REST URL such as `https://host/ords/health/health_reports/` belongs to.
pub fn sql_url(base_url: &str) -> Option<String> {
    let after = base_url.split_once("/ords/")?;
    let alias = after.1.split('/').next().filter(|a| !a.is_empty())?;
    Some(format!("{}/ords/{alias}/_/sql", after.0))
}

/// A value as a SQL string literal: quotes doubled, and `&` replaced, which the SQL endpoint would read as a substitution variable.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''").replace('&', "_"))
}

/// One `MERGE` that writes the samples, a later one at the same instant replacing an earlier. Non-finite values are left out.
pub fn merge_statement(samples: &[Sample]) -> Option<String> {
    let mut latest: BTreeMap<(i64, &str, &str, &str), f64> = BTreeMap::new();
    for sample in samples.iter().filter(|s| s.value.is_finite()) {
        latest.insert((sample.ts, &sample.scope, &sample.name, &sample.metric), sample.value);
    }
    if latest.is_empty() {
        return None;
    }
    let rows: Vec<String> = latest
        .iter()
        .map(|((ts, scope, name, metric), value)| format!("SELECT {ts} TS, {} SCOPE, {} NAME, {} METRIC, TO_BINARY_DOUBLE('{value}') VALUE FROM DUAL", literal(scope), literal(name), literal(metric)))
        .collect();
    Some(format!(
        "MERGE INTO HOST_SAMPLES T USING ({}) S ON (T.TS = S.TS AND T.SCOPE = S.SCOPE AND T.NAME = S.NAME AND T.METRIC = S.METRIC)
         WHEN MATCHED THEN UPDATE SET T.VALUE = S.VALUE
         WHEN NOT MATCHED THEN INSERT (TS, SCOPE, NAME, METRIC, VALUE) VALUES (S.TS, S.SCOPE, S.NAME, S.METRIC, S.VALUE)",
        rows.join(" UNION ALL ")
    ))
}

/// The points at or after `cutoff`: the rows as stored for a raw window, else the averages per `bucket_ms`.
pub fn series_statement(cutoff: i64, bucket_ms: i64, offset: usize, query: &SeriesQuery) -> String {
    let cutoff = query.since_ms.map_or(cutoff, |since| cutoff.max(since));
    let mut scope = query.scope.as_deref().map_or(String::new(), |s| format!(" AND SCOPE = {}", literal(s)));
    if !query.metrics.is_empty() {
        scope.push_str(&format!(" AND METRIC IN ({})", query.metrics.iter().map(|m| literal(m)).collect::<Vec<_>>().join(", ")));
    }
    let page = format!("OFFSET {offset} ROWS FETCH NEXT {PAGE} ROWS ONLY");
    if bucket_ms == 0 {
        format!("SELECT TS, SCOPE, NAME, METRIC, VALUE FROM HOST_SAMPLES WHERE TS >= {cutoff}{scope} ORDER BY TS, SCOPE, NAME, METRIC {page}")
    } else {
        format!(
            "SELECT FLOOR(TS / {bucket_ms}) * {bucket_ms} AS TS, SCOPE, NAME, METRIC, AVG(VALUE) AS VALUE FROM HOST_SAMPLES WHERE TS >= {cutoff}{scope} \
             GROUP BY FLOOR(TS / {bucket_ms}) * {bucket_ms}, SCOPE, NAME, METRIC ORDER BY 1, 2, 3, 4 {page}"
        )
    }
}

/// The newest sample of every series seen in the last day of stored history.
pub const LATEST_STATEMENT: &str = "SELECT MAX(TS) AS TS, SCOPE, NAME, METRIC, MAX(VALUE) KEEP (DENSE_RANK LAST ORDER BY TS) AS VALUE FROM HOST_SAMPLES \
     WHERE TS >= (SELECT NVL(MAX(TS), 0) - 86400000 FROM HOST_SAMPLES) GROUP BY SCOPE, NAME, METRIC";

pub fn delete_oldest_statement(rows: u64) -> String {
    format!("DELETE FROM HOST_SAMPLES WHERE TS <= (SELECT MAX(TS) FROM (SELECT TS FROM HOST_SAMPLES ORDER BY TS FETCH FIRST {rows} ROWS ONLY))")
}

pub const SHRINK_STATEMENT: &str = "ALTER TABLE HOST_SAMPLES SHRINK SPACE CASCADE";
pub const USED_STATEMENT: &str = "SELECT NVL(SUM(BYTES), 0) AS BYTES FROM USER_SEGMENTS";
pub const COUNT_STATEMENT: &str = "SELECT COUNT(*) AS N FROM HOST_SAMPLES";

/// The rows of the first result set an SQL-endpoint reply holds, or the error it carries.
fn rows_of(reply: &Value) -> Result<Vec<Value>, PortError> {
    let item = reply["items"].get(0).ok_or_else(|| PortError::new("ords: the reply has no items"))?;
    // `result` is 0 for a query and the number of rows for DML, so a failure is told by `errorDetails` alone.
    if item.get("errorDetails").is_some() {
        let text = item["errorDetails"].as_str().or_else(|| item["response"].get(0).and_then(Value::as_str)).unwrap_or("the statement failed");
        return Err(PortError::new(format!("ords: {}", text.lines().find(|l| !l.trim().is_empty()).unwrap_or(text).trim())));
    }
    Ok(item["resultSet"]["items"].as_array().cloned().unwrap_or_default())
}

/// The number a DML reply reports ("3 rows deleted."), 0 when it says none.
fn affected(reply: &Value) -> u64 {
    reply["items"][0]["response"].get(0).and_then(Value::as_str).and_then(|text| text.split_whitespace().find_map(|word| word.parse().ok())).unwrap_or(0)
}

fn number(value: &Value, key: &str) -> Result<f64, PortError> {
    value[key].as_f64().ok_or_else(|| PortError::new(format!("ords: row without a numeric {key}")))
}

fn text(value: &Value, key: &str) -> Result<String, PortError> {
    value[key].as_str().map(String::from).ok_or_else(|| PortError::new(format!("ords: row without a text {key}")))
}

fn sample_of(row: &Value) -> Result<Sample, PortError> {
    Ok(Sample { ts: number(row, "ts")? as i64, scope: text(row, "scope")?, name: text(row, "name")?, metric: text(row, "metric")?, value: number(row, "value")? })
}

pub struct OrdsStore {
    client: reqwest::Client,
    url: String,
    user: String,
    password: String,
    pending: Mutex<VecDeque<Sample>>,
    count: Mutex<Option<(u64, Instant)>>,
}

impl OrdsStore {
    /// Connects and makes sure the table exists; the service refuses to start when the database does not answer at all.
    pub async fn connect(base_url: &str, user: &str, password: &str) -> Result<Self, PortError> {
        let url = sql_url(base_url).ok_or_else(|| PortError::new(format!("not an ORDS auto-REST url: {base_url}")))?;
        let client = reqwest::Client::builder().timeout(Duration::from_secs(60)).build().map_err(PortError::new)?;
        let store = Self { client, url, user: user.into(), password: password.into(), pending: Mutex::new(VecDeque::new()), count: Mutex::new(None) };
        for statement in SCHEMA {
            store.run(statement).await?;
        }
        Ok(store)
    }

    async fn run(&self, statement: &str) -> Result<Value, PortError> {
        let response = self
            .client
            .post(&self.url)
            .basic_auth(&self.user, Some(&self.password))
            .header(reqwest::header::CONTENT_TYPE, "application/sql")
            .header(reqwest::header::ACCEPT, "application/json")
            .body(statement.to_string())
            .send()
            .await
            .map_err(PortError::new)?;
        let status = response.status();
        let body = response.text().await.map_err(PortError::new)?;
        if !status.is_success() {
            return Err(PortError::new(format!("ords answered {}", status.as_u16())));
        }
        serde_json::from_str(&body).map_err(PortError::new)
    }

    async fn query(&self, statement: &str) -> Result<Vec<Value>, PortError> {
        rows_of(&self.run(statement).await?)
    }

    async fn scalar(&self, statement: &str, key: &str) -> Result<u64, PortError> {
        let rows = self.query(statement).await?;
        Ok(rows.first().map(|row| number(row, key)).transpose()?.unwrap_or(0.0) as u64)
    }

    /// Writes what is waiting, a chunk at a time; what could not be written stays waiting for the next sweep.
    async fn flush(&self) -> Result<(), PortError> {
        loop {
            let chunk: Vec<Sample> = {
                let pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
                pending.iter().take(CHUNK).cloned().collect()
            };
            if chunk.is_empty() {
                return Ok(());
            }
            if let Some(statement) = merge_statement(&chunk) {
                rows_of(&self.run(&statement).await?)?;
            }
            let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            let written = chunk.len().min(pending.len());
            pending.drain(..written);
        }
    }
}

#[async_trait]
impl MetricsStore for OrdsStore {
    async fn insert_samples(&self, samples: Vec<Sample>) -> Result<(), PortError> {
        {
            let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            pending.extend(samples);
            let excess = pending.len().saturating_sub(BUFFER_CAP);
            pending.drain(..excess);
        }
        self.flush().await
    }

    async fn series_for(&self, window_ms: i64, now: i64, query: &SeriesQuery) -> Result<Vec<SeriesPoint>, PortError> {
        let (granularity, bucket_ms) = granularity_for(window_ms);
        let cutoff = now - window_ms;
        let mut points = Vec::new();
        loop {
            let rows = self.query(&series_statement(cutoff, bucket_ms, points.len(), query)).await?;
            let page = rows.len();
            for row in &rows {
                points.push(SeriesPoint { sample: sample_of(row)?, granularity });
            }
            if page < PAGE {
                return Ok(points);
            }
        }
    }

    async fn latest(&self) -> Result<Vec<Sample>, PortError> {
        self.query(LATEST_STATEMENT).await?.iter().map(sample_of).collect()
    }

    async fn row_count(&self) -> Result<u64, PortError> {
        if let Some((rows, at)) = *self.count.lock().unwrap_or_else(|p| p.into_inner()) {
            if at.elapsed() < COUNT_TTL {
                return Ok(rows);
            }
        }
        let fresh = self.scalar(COUNT_STATEMENT, "n").await;
        let mut count = self.count.lock().unwrap_or_else(|p| p.into_inner());
        match (fresh, *count) {
            (Ok(rows), _) => {
                *count = Some((rows, Instant::now()));
                Ok(rows)
            }
            // The database being unreachable must not turn the container unhealthy; the count is a courtesy.
            (Err(error), Some((rows, _))) => {
                tracing::warn!(error = %error, "store.row_count.stale");
                Ok(rows)
            }
            (Err(error), None) => Err(error),
        }
    }

    async fn used_bytes(&self) -> Result<u64, PortError> {
        self.scalar(USED_STATEMENT, "bytes").await
    }

    async fn delete_oldest(&self, rows: u64) -> Result<u64, PortError> {
        let reply = self.run(&delete_oldest_statement(rows)).await?;
        rows_of(&reply)?;
        let deleted = affected(&reply);
        rows_of(&self.run(SHRINK_STATEMENT).await?)?;
        *self.count.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_an_auto_rest_url_when_the_sql_endpoint_is_derived_then_it_is_the_schema_aliass() {
        assert_eq!(sql_url("https://abc.adb.eu-frankfurt-1.oraclecloudapps.com/ords/health/health_reports/").as_deref(), Some("https://abc.adb.eu-frankfurt-1.oraclecloudapps.com/ords/health/_/sql"));
        assert_eq!(sql_url("https://example.com/other"), None);
    }

    #[test]
    fn given_hostile_names_when_samples_are_written_then_the_statement_cannot_be_broken_out_of_or_read_as_a_substitution() {
        let sql = merge_statement(&[Sample::new(1, "host", "o'brien&co", "cpu'); DROP TABLE HOST_SAMPLES; --", 1.0)]).unwrap();
        assert_eq!(sql.matches('\'').count() % 2, 0, "every quote is closed or doubled: {sql}");
        assert!(!sql.contains('&'), "the SQL endpoint would read & as a substitution variable: {sql}");
    }

    #[test]
    fn given_only_non_finite_values_when_samples_are_written_then_there_is_nothing_to_send() {
        assert_eq!(merge_statement(&[Sample::new(1, "a", "b", "c", f64::INFINITY), Sample::new(2, "a", "b", "c", f64::NAN)]), None);
    }

    #[test]
    fn given_replies_when_read_then_rows_come_out_and_failures_name_their_cause() {
        let ok = json!({ "items": [{ "result": 0, "resultSet": { "items": [{ "ts": 5, "scope": "host", "name": "h", "metric": "cpu", "value": 1.5 }] } }] });
        assert_eq!(sample_of(&rows_of(&ok).unwrap()[0]).unwrap(), Sample::new(5, "host", "h", "cpu", 1.5));
        let bad = json!({ "items": [{ "result": 1, "errorDetails": "\nORA-00942: table or view does not exist\nmore" }] });
        assert_eq!(rows_of(&bad).unwrap_err().0, "ords: ORA-00942: table or view does not exist");
        let dml = json!({ "items": [{ "result": 37, "response": ["\n37 rows deleted.\n\n"] }] });
        assert!(rows_of(&dml).unwrap().is_empty(), "a DML reply whose result is its row count is not a failure");
        assert_eq!(affected(&dml), 37);
    }
}
