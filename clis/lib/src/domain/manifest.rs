//! `Cargo.toml` edits that keep every comment and line the edit does not mean to change.
use super::version::{bump, Bump};
use toml_edit::{value, DocumentMut, InlineTable, Item, TableLike, Value};

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|error| format!("not a Cargo.toml: {error}"))
}

/// `(old, new, text)` with `[package] version` raised by `how`. A version inherited from the workspace has no line to raise.
pub fn bump_version(text: &str, how: Bump) -> Result<(String, String, String), String> {
    let mut doc = parse(text)?;
    let slot = doc.get_mut("package").and_then(|p| p.get_mut("version")).and_then(Item::as_value_mut).ok_or("[package] version is not a plain string (a `version.workspace = true` crate is not released by this verb)")?;
    let old = slot.as_str().ok_or("[package] version is not a string")?.to_string();
    let new = bump(&old, how)?;
    let decor = slot.decor().clone();
    *slot = Value::from(new.as_str());
    *slot.decor_mut() = decor;
    Ok((old, new, doc.to_string()))
}

/// Where a dependency line points: crates.io (no `registry` key) or a named alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    CratesIo,
    Alias(String),
}

pub const CRATES_IO: &str = "crates-io";

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Alias(alias) => alias,
            Target::CratesIo => CRATES_IO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub table: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub table: String,
    pub why: String,
}

#[derive(Default)]
pub struct Retargeted {
    pub text: String,
    pub changed: Vec<Change>,
    pub skipped: Vec<Skip>,
}

const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// `text` with every dependency on `name` (by key or by `package =`) moved to `to`: `registry = "mla"` added or removed, and a
/// bare `name = "1.2.3"` written as a table when it needs the registry key.
pub fn retarget(text: &str, name: &str, to: &Target) -> Result<Retargeted, String> {
    let mut doc = parse(text)?;
    let mut out = Retargeted::default();
    visit(doc.as_table_mut(), "", name, to, &mut out);
    out.text = doc.to_string();
    Ok(out)
}

fn visit(table: &mut dyn TableLike, at: &str, name: &str, to: &Target, out: &mut Retargeted) {
    for kind in KINDS {
        if let Some(deps) = table.get_mut(kind).and_then(Item::as_table_like_mut) {
            let label = format!("{at}{kind}");
            let keys: Vec<String> = deps.iter().map(|(k, _)| k.to_string()).collect();
            for key in keys {
                if let Some(item) = deps.get_mut(&key) {
                    dependency(item, &key, &label, name, to, out);
                }
            }
        }
    }
    if let Some(workspace) = table.get_mut("workspace").and_then(Item::as_table_like_mut) {
        visit(workspace, "workspace.", name, to, out);
    }
    if let Some(targets) = table.get_mut("target").and_then(Item::as_table_like_mut) {
        let cfgs: Vec<String> = targets.iter().map(|(k, _)| k.to_string()).collect();
        for cfg in cfgs {
            if let Some(inner) = targets.get_mut(&cfg).and_then(Item::as_table_like_mut) {
                visit(inner, &format!("target.{cfg}."), name, to, out);
            }
        }
    }
}

fn registry_of(item: &Item) -> String {
    item.as_table_like().and_then(|t| t.get("registry")).and_then(Item::as_str).unwrap_or(CRATES_IO).to_string()
}

fn dependency(item: &mut Item, key: &str, table: &str, name: &str, to: &Target, out: &mut Retargeted) {
    let renamed = item.as_table_like().and_then(|t| t.get("package")).and_then(Item::as_str).map(str::to_string);
    if renamed.as_deref().unwrap_or(key) != name {
        return;
    }
    let skip = |out: &mut Retargeted, why: &str| out.skipped.push(Skip { table: table.to_string(), why: format!("{key}: {why}") });
    let from = registry_of(item);
    if let Some(bare) = item.as_str().map(str::to_string) {
        if *to != Target::CratesIo {
            let mut inline = InlineTable::new();
            inline.insert("version", Value::from(bare));
            inline.insert("registry", Value::from(to.name()));
            *item = Item::Value(Value::InlineTable(inline));
            out.changed.push(Change { table: table.to_string(), from: from.clone(), to: to.name().into() });
        }
        return;
    }
    let Some(entry) = item.as_table_like_mut() else { return };
    if entry.get("workspace").is_some() {
        return skip(out, "inherits the workspace's dependency line, which is retargeted itself");
    }
    if entry.get("git").is_some() || (entry.get("path").is_some() && entry.get("version").is_none()) {
        return skip(out, "a path or git dependency has no registry to move");
    }
    if from == to.name() {
        return;
    }
    match to {
        Target::Alias(alias) => {
            entry.insert("registry", value(alias.as_str()));
        }
        Target::CratesIo => {
            entry.remove("registry");
        }
    }
    if let Some(inline) = item.as_inline_table_mut() {
        inline.fmt();
    }
    let Some(entry) = item.as_table_like_mut() else { return };
    let only_version = entry.len() == 1 && entry.get("version").is_some_and(Item::is_str);
    if only_version && *to == Target::CratesIo && item.is_inline_table() {
        let version = item.as_table_like().and_then(|t| t.get("version")).and_then(Item::as_str).unwrap_or_default().to_string();
        *item = value(version);
    }
    out.changed.push(Change { table: table.to_string(), from: from.clone(), to: to.name().into() });
}

/// A `.cargo/config.toml` with `[registries.<alias>] index` set, every other line kept.
pub fn with_registry_index(text: &str, alias: &str, index: &str) -> Result<String, String> {
    let mut doc = parse(text).map_err(|e| e.replace("Cargo.toml", ".cargo/config.toml"))?;
    let registries = doc.entry("registries").or_insert_with(|| Item::Table(toml_edit::Table::new())).as_table_like_mut().ok_or("[registries] is not a table")?;
    let one = registries.entry(alias).or_insert_with(|| Item::Table(toml_edit::Table::new())).as_table_like_mut().ok_or("[registries.<alias>] is not a table")?;
    one.insert("index", value(index));
    Ok(doc.to_string())
}
