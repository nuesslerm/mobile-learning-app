//! The checked-in method list of one app (`docs/plans/rca-parity/coverage/<app>-methods.json`): every row the reference scan
//! found, whether the phone reaches it, and where.
use serde_json::{json, Map, Value};

pub const SCHEMA: u64 = 1;
/// Call sites kept per row: enough to find the use, small enough to review.
pub const SITES_KEPT: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// `<kind>:<name>`, the key decisions and coverage use.
    pub id: String,
    pub kind: String,
    pub name: String,
    pub phone: bool,
    pub subscription: bool,
    /// `flagged` or `offAllowlist` (Orca), else none.
    pub class: Option<String>,
    /// Kind-specific facts (an HTTP row's method and path, an Orca method's allowlist route and flag).
    pub extra: Map<String, Value>,
    pub sites: Vec<String>,
}

impl Row {
    pub fn new(kind: &str, name: &str) -> Row {
        Row { id: format!("{kind}:{name}"), kind: kind.into(), name: name.into(), phone: false, subscription: false, class: None, extra: Map::new(), sites: vec![] }
    }

    pub fn reached(&mut self, site: String) {
        self.phone = true;
        if !self.sites.contains(&site) {
            self.sites.push(site);
            self.sites.sort();
        }
    }

    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), self.id.clone().into());
        o.insert("kind".into(), self.kind.clone().into());
        o.insert("name".into(), self.name.clone().into());
        o.insert("phone".into(), self.phone.into());
        if self.subscription {
            o.insert("subscription".into(), true.into());
        }
        if let Some(c) = &self.class {
            o.insert("class".into(), c.clone().into());
        }
        o.extend(self.extra.clone());
        if !self.sites.is_empty() {
            o.insert("sites".into(), self.sites.iter().take(SITES_KEPT).cloned().collect::<Vec<_>>().into());
        }
        Value::Object(o)
    }

    fn from_json(v: &Value) -> Option<Row> {
        let o = v.as_object()?;
        let known = ["id", "kind", "name", "phone", "subscription", "class", "sites"];
        Some(Row {
            id: o.get("id")?.as_str()?.into(),
            kind: o.get("kind")?.as_str()?.into(),
            name: o.get("name")?.as_str()?.into(),
            phone: o.get("phone").and_then(Value::as_bool).unwrap_or(false),
            subscription: o.get("subscription").and_then(Value::as_bool).unwrap_or(false),
            class: o.get("class").and_then(Value::as_str).map(String::from),
            extra: o.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect(),
            sites: o.get("sites").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodsList {
    pub app: String,
    /// The pin the scan read: `{file, version, commit}`.
    pub pin: Value,
    /// Catalog and phone counts, as the scan measured them.
    pub counts: Value,
    pub rows: Vec<Row>,
    /// What the scan saw and could not read as rows: dynamic method names, native modules, raw fetches.
    pub flags: Value,
}

impl MethodsList {
    pub fn to_text(&self) -> String {
        let doc = json!({
            "schema": SCHEMA,
            "app": self.app,
            "generatedBy": format!("wf parity coverage --app {}", self.app),
            "pin": self.pin,
            "counts": self.counts,
            "flags": self.flags,
            "rows": self.rows.iter().map(Row::to_json).collect::<Vec<_>>(),
        });
        pretty(&doc)
    }

    pub fn parse(text: &str) -> Result<MethodsList, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        if v["schema"].as_u64() != Some(SCHEMA) {
            return Err(format!("schema {} is not {SCHEMA}", v["schema"]));
        }
        let rows = v["rows"].as_array().ok_or("no rows")?.iter().map(|r| Row::from_json(r).ok_or_else(|| format!("a malformed row: {r}"))).collect::<Result<Vec<_>, _>>()?;
        Ok(MethodsList { app: v["app"].as_str().unwrap_or_default().into(), pin: v["pin"].clone(), counts: v["counts"].clone(), rows, flags: v["flags"].clone() })
    }

    pub fn phone_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.phone)
    }
}

/// Two-space JSON with a final newline, the form every checked-in file of this crate takes.
pub fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_back_as_it_was_written() {
        // Given a list with a phone row carrying extra facts and more sites than are kept
        let mut row = Row::new("http", "auth.session");
        row.extra.insert("method".into(), "GET".into());
        for s in ["d.ts:4", "a.ts:1", "c.ts:3", "b.ts:2"] {
            row.reached(s.into());
        }
        let list = MethodsList { app: "t3".into(), pin: json!({"commit": "abc"}), counts: json!({"http": 1}), rows: vec![row, Row::new("rpc", "x.y")], flags: json!({}) };
        // When it is written and parsed
        let back = MethodsList::parse(&list.to_text()).unwrap();
        // Then the rows come back, keeping the first sites in order
        assert_eq!(back.rows[0].sites, ["a.ts:1", "b.ts:2", "c.ts:3"]);
        assert_eq!(back.rows[0].extra["method"], "GET");
        assert_eq!(back.phone_rows().count(), 1);
        assert_eq!(back.to_text(), MethodsList::parse(&back.to_text()).unwrap().to_text());
    }
}
