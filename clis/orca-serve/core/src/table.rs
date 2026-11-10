//! The method table, assembled from the areas: each method name exactly once, served or not offered.
use crate::area::{Area, Kind};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Entry {
    Served { area: usize, kind: Kind },
    NotOffered { reason: &'static str, decided_by: &'static str },
}

#[derive(Debug, Default)]
pub struct Table {
    entries: BTreeMap<&'static str, Entry>,
    capabilities: Vec<String>,
}

impl Table {
    /// Refuses a name two areas claim, or one an area both serves and marks not offered.
    pub fn assemble(areas: &[Box<dyn Area>]) -> Result<Table, String> {
        let mut table = Table::default();
        for (index, area) in areas.iter().enumerate() {
            for m in area.methods() {
                table.insert(m.name, Entry::Served { area: index, kind: m.kind }, area.name())?;
            }
            for n in area.not_offered() {
                table.insert(n.name, Entry::NotOffered { reason: n.reason, decided_by: n.decided_by }, area.name())?;
            }
            table.capabilities.extend(area.capabilities().iter().map(|c| c.to_string()));
        }
        Ok(table)
    }

    fn insert(&mut self, name: &'static str, entry: Entry, area: &str) -> Result<(), String> {
        match self.entries.insert(name, entry) {
            None => Ok(()),
            Some(_) => Err(format!("method {name} is declared twice (again by area {area})")),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// The table as `orca-serve --api` prints it: every method with its state, sorted by name.
    pub fn api(&self, areas: &[Box<dyn Area>]) -> Value {
        let rows: Vec<Value> = self
            .entries
            .iter()
            .map(|(name, entry)| match entry {
                Entry::Served { area, kind } => json!({"method": name, "state": "served", "area": areas[*area].name(), "streaming": *kind == Kind::Streaming}),
                Entry::NotOffered { reason, decided_by } => json!({"method": name, "state": "not-offered", "reason": reason, "decidedBy": decided_by}),
            })
            .collect();
        json!({"schema": 1, "methods": rows, "capabilities": self.capabilities})
    }
}
