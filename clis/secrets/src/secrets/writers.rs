//! `wf secret set|delete|push-notes|create|update|rename|move`: the vault and keychain writers. Dry by default where the
//! TypeScript is (`--apply` writes); every vault write goes through `vault.rs` with the edit credential only.
use super::registry::Entry;
use super::util::{mask, PLACEHOLDER_VALUE};
use super::vault::{self, record_history};
use super::{err, out, Ctx};
use wf_kit::args::Parsed;
use std::io::{IsTerminal, Read};

const DEFAULT_PROJECT_ID: &str = "65501031-2ee6-4d57-9e45-b48e0175233f";

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

fn has(p: &Parsed, flag: &str) -> bool {
    p.flags.iter().any(|f| f == flag)
}

/// A thrown error the TypeScript catches and prints plainly, then exits 1.
fn plain(message: &str) -> i32 {
    err(&format!("{message}\n"));
    1
}

fn read_stdin() -> String {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    raw
}

fn stdin_value(key: &str) -> Result<String, String> {
    let trimmed = read_stdin().trim().to_string();
    if trimmed.is_empty() {
        return Err(format!("{key}: stdin closed with no value provided"));
    }
    Ok(trimmed)
}

fn entry_of(ctx: &Ctx, key: &str) -> Option<Entry> {
    ctx.registry.iter().find(|e| e.key == key).cloned()
}

/* ---------- set / delete ---------- */

pub fn cmd_set(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(None)?;
    let key = &p.positional[0];
    let Some(entry) = entry_of(&ctx, key) else { return Ok(ctx.unknown_key(key)) };
    let value = match p.positional.get(1) {
        Some(v) => v.clone(),
        None if std::io::stdin().is_terminal() => {
            return Ok(plain(
                &format!("{key}: no value given. Provide it as a positional arg (wf secret set {key} <value>) or pipe one via stdin (echo -n '<value>' | wf secret set {key})."),
            ))
        }
        None => match stdin_value(key) {
            Ok(v) => v,
            Err(m) => return Ok(plain(&m)),
        },
    };
    // The value being overwritten is archived first; a capture failure never blocks the write.
    let previous = ctx.backend.resolve(&entry).ok().flatten();
    record_history(&entry.env_var, ctx.backend.name(), previous.as_deref());
    if ctx.backend.name() == "bitwarden" {
        let writer = vault::vault_writer()?;
        match writer.list()?.into_iter().find(|s| s.key == entry.env_var) {
            Some(existing) => writer.update_value(&existing.id, &value)?,
            None => writer.create(&entry.env_var, &value, DEFAULT_PROJECT_ID, &entry.description)?,
        }
    } else {
        ctx.backend.store(&entry.service, &value)?;
    }
    out(&format!("{key}: stored\n"));
    Ok(0)
}

pub fn cmd_delete(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(None)?;
    let key = &p.positional[0];
    let Some(entry) = entry_of(&ctx, key) else { return Ok(ctx.unknown_key(key)) };
    ctx.backend.remove(&entry.service)?;
    out(&format!("{key}: deleted\n"));
    Ok(0)
}

/* ---------- push-notes ---------- */

type Planned = (String, Option<String>, &'static str, String, String);

pub fn cmd_push_notes(p: Parsed) -> Result<i32, String> {
    let ctx = Ctx::new(Some("bitwarden"))?;
    let (apply, verbose) = (has(&p, "apply"), has(&p, "verbose"));
    let planned: Result<Vec<Planned>, String> = (|| {
        let writer = vault::note_writer()?;
        let listed = writer.list()?;
        let by_key: std::collections::HashMap<&str, &vault::VaultNote> = listed.iter().map(|s| (s.key.as_str(), s)).collect();
        let plan: Vec<_> = ctx
            .registry
            .iter()
            .map(|e| {
                let vault_key = e.bitwarden_key.as_deref().unwrap_or(&e.env_var);
                match by_key.get(vault_key) {
                    None => (e.key.clone(), None, "absent-from-vault", String::new(), e.description.clone()),
                    Some(found) => {
                        let action = if found.note == e.description { "unchanged" } else { "update" };
                        (e.key.clone(), Some(found.id.clone()), action, found.note.clone(), e.description.clone())
                    }
                }
            })
            .collect();
        if apply {
            for (_, id, action, _, desired) in &plan {
                if let (Some(id), "update") = (id, *action) {
                    writer.set_note(id, desired)?;
                }
            }
        }
        Ok(plan)
    })();
    let plan = match planned {
        Ok(p) => p,
        Err(m) => return Ok(plain(&m)),
    };
    let updates: Vec<_> = plan.iter().filter(|p| p.2 == "update").collect();
    let unchanged = plan.iter().filter(|p| p.2 == "unchanged").count();
    let absent: Vec<_> = plan.iter().filter(|p| p.2 == "absent-from-vault").collect();
    for u in &updates {
        out(&format!("{}  {}  ({} -> {} chars)\n", if apply { "updated" } else { "would update" }, u.0, units(&u.3), units(&u.4)));
    }
    if verbose {
        for a in &absent {
            out(&format!("skipped       {}  (registered here, not in the vault)\n", a.0));
        }
    }
    out(&format!(
        "\n{} {} note(s); {unchanged} already current; {} not in the vault.\n",
        if apply { "Wrote" } else { "Would write" },
        updates.len(),
        absent.len()
    ));
    if !absent.is_empty() && !verbose {
        out("Re-run with --verbose to list the keys absent from the vault.\n");
    }
    if !apply && !updates.is_empty() {
        out("Dry run — re-run with --apply to write.\n");
    }
    Ok(0)
}

/* ---------- create / update / rename ---------- */

fn explain_refusal(refusal: &str, action: &str, key: &str, from_key: &str) -> String {
    match refusal {
        "target-key-exists" => format!("a secret named {key} already exists in the vault — refusing to create a duplicate"),
        "source-missing" => format!("no vault secret named {from_key} — nothing to rename"),
        "unregistered-key" => format!("{key} is not in registry.json, so it has no reviewed description to use as its note — add the registry entry first"),
        _ => {
            let _ = action;
            String::new()
        }
    }
}

pub fn cmd_create(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(Some("bitwarden"))?;
    let (key, project_id) = (p.positional[0].clone(), p.positional[1].clone());
    let non_empty = |name: &str| p.opts.get(name).filter(|v| !v.is_empty()).cloned();
    let (value_opt, value_from) = (non_empty("value"), non_empty("value-from"));
    let (stdin_flag, placeholder, apply) = (has(&p, "value-stdin"), has(&p, "placeholder"), has(&p, "apply"));
    let sources = [value_opt.is_some(), stdin_flag, value_from.is_some(), placeholder].iter().filter(|b| **b).count();
    if sources != 1 {
        return Ok(plain("Pass exactly one of --value, --value-stdin, --value-from <KEY>, or --placeholder"));
    }
    let result: Result<i32, String> = (|| {
        let value = if placeholder {
            PLACEHOLDER_VALUE.to_string()
        } else if stdin_flag {
            match stdin_value(&key) {
                Ok(v) => v,
                Err(m) => return Ok(plain(&m)),
            }
        } else if let Some(from_key) = &value_from {
            let Some(from_entry) = entry_of(&ctx, from_key) else { return Ok(ctx.unknown_key(from_key)) };
            match ctx.get(&from_entry)? {
                Some(v) if !v.is_empty() => v,
                _ => return Ok(plain(&format!("--value-from {from_key} resolved to nothing; refusing to create an empty secret"))),
            }
        } else {
            value_opt.clone().unwrap_or_default()
        };
        let writer = vault::vault_writer()?;
        let vault_list = writer.list()?;
        let registered = entry_of(&ctx, &key);
        let (note, refusal) = match &registered {
            None => (String::new(), Some("unregistered-key")),
            Some(e) if vault_list.iter().any(|s| s.key == key) => (e.description.clone(), Some("target-key-exists")),
            Some(e) => (e.description.clone(), None),
        };
        if let Some(refusal) = refusal {
            err(&format!("Refused: {}\n", explain_refusal(refusal, "create", &key, "")));
            return Ok(1);
        }
        if apply {
            writer.create(&key, &value, &project_id, &note)?;
        }
        out(&format!("{}  {key}  in project {project_id}  (note: {} chars)\n", if apply { "created" } else { "would create" }, units(&note)));
        if !apply {
            out("Dry run — re-run with --apply to write.\n");
        }
        Ok(0)
    })();
    match result {
        Ok(code) => Ok(code),
        Err(m) => Ok(plain(&m)),
    }
}

/// Writes `value` into the vault under the entry's existing key, archiving the previous value first. Refuses a key
/// the vault does not hold, so a rotation can never invent one.
fn write_vault_value(ctx: &mut Ctx, entry: &Entry, key: &str, value: &str) -> Result<(bool, std::path::PathBuf), String> {
    let vault_key = entry.bitwarden_key.clone().unwrap_or_else(|| entry.env_var.clone());
    let writer = vault::vault_writer()?;
    let found = writer.list()?.into_iter().find(|s| s.key == vault_key).ok_or_else(|| {
        format!("{key} resolves to vault key {vault_key}, which does not exist — refusing to rotate a secret that is not there. Create it first (`wf secret create`), so a rotation can never silently invent one.")
    })?;
    let previous = ctx.backend.resolve(entry).ok().flatten();
    let captured = record_history(&vault_key, ctx.backend.name(), previous.as_deref());
    writer.update_value(&found.id, value)?;
    Ok(captured)
}

/// Stores `value` under the registry key `key` in the vault (the write half of `cmd_update --apply`).
pub fn update_secret_value(key: &str, value: &str) -> Result<(), String> {
    let mut ctx = Ctx::new(Some("bitwarden"))?;
    let entry = entry_of(&ctx, key).ok_or_else(|| format!("unknown secret key {key}"))?;
    let name = entry.key.clone();
    write_vault_value(&mut ctx, &entry, &name, value).map(|_| ())
}

pub fn cmd_update(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(Some("bitwarden"))?;
    let raw_key = &p.positional[0];
    let Some(entry) = entry_of(&ctx, raw_key) else { return Ok(ctx.unknown_key(raw_key)) };
    let key = entry.key.clone();
    let value_opt = p.opts.get("value").filter(|v| !v.is_empty()).cloned();
    let stdin_flag = has(&p, "value-stdin");
    if usize::from(value_opt.is_some()) + usize::from(stdin_flag) != 1 {
        return Ok(plain("Pass exactly one of --value or --value-stdin"));
    }
    let value = if stdin_flag {
        match stdin_value(&key) {
            Ok(v) => v,
            Err(m) => return Ok(plain(&m)),
        }
    } else {
        value_opt.unwrap_or_default()
    };
    if !has(&p, "apply") {
        out(&format!("would update  {key}  to a {}-char value ({})\nDry run — re-run with --apply to write.\n", units(&value), mask(&value)));
        return Ok(0);
    }
    let rotated = write_vault_value(&mut ctx, &entry, &key, &value);
    let (captured, path) = match rotated {
        Ok(r) => r,
        Err(m) => {
            err(&format!("{m}\n"));
            return Ok(1);
        }
    };
    out(&format!("updated  {key}  ({} chars, {})\n", units(&value), mask(&value)));
    if captured {
        out(&format!("previous value archived to {} (owner-only file, value never printed — for emergency recovery only)\n", path.display()));
    } else {
        err(&format!("WARNING: could not archive the previous value to {} — it is now unrecoverable if this rotation needs to be rolled back\n", path.display()));
    }
    Ok(0)
}

pub fn cmd_move(p: Parsed) -> Result<i32, String> {
    let (key, project_id) = (p.positional[0].clone(), p.positional[1].clone());
    let apply = has(&p, "apply");
    let result: Result<i32, String> = (|| {
        let writer = vault::vault_writer()?;
        let listed = writer.list()?;
        let matches: Vec<_> = listed.iter().filter(|s| s.key == key).collect();
        let source = match matches.as_slice() {
            [] => return Ok(plain(&format!("Refused: no vault secret named {key} — nothing to move"))),
            [one] => one,
            many => return Ok(plain(&format!("Refused: {} vault secrets are named {key} — move one by hand in the web vault", many.len()))),
        };
        if source.project_id == project_id {
            out(&format!("{key} is already in project {project_id} — nothing to move\n"));
            return Ok(0);
        }
        if apply {
            writer.move_to(&source.id, &project_id)?;
        }
        out(&format!("{}  {key}  {} -> {project_id}  (id preserved)\n", if apply { "moved" } else { "would move" }, source.project_id));
        if !apply {
            out("Dry run — re-run with --apply to write.\n");
        }
        Ok(0)
    })();
    match result {
        Ok(code) => Ok(code),
        Err(m) => Ok(plain(&m)),
    }
}

pub fn cmd_rename(p: Parsed) -> Result<i32, String> {
    let mut ctx = Ctx::new(Some("bitwarden"))?;
    let (from_key, to_key) = (p.positional[0].clone(), p.positional[1].clone());
    let (additive, apply) = (has(&p, "additive"), has(&p, "apply"));
    let result: Result<i32, String> = (|| {
        let mut value: Option<String> = None;
        if additive {
            let lookup = if ctx.valid_key(&from_key).is_some() { from_key.clone() } else { to_key.clone() };
            let Some(entry) = entry_of(&ctx, &lookup) else { return Ok(ctx.unknown_key(&lookup)) };
            match ctx.get(&entry)? {
                Some(v) if !v.is_empty() => value = Some(v),
                _ => {
                    return Ok(plain(
                        &format!("--additive needs the current value of {from_key}, but it resolved to nothing. Refusing to create an empty copy."),
                    ))
                }
            }
        }
        let writer = vault::vault_writer()?;
        let listed = writer.list()?;
        let source = listed.iter().find(|s| s.key == from_key);
        let refusal = if source.is_none() {
            Some("source-missing")
        } else if listed.iter().any(|s| s.key == to_key) {
            Some("target-key-exists")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            err(&format!("Refused: {}\n", explain_refusal(refusal, "rename", &to_key, &from_key)));
            return Ok(1);
        }
        let source = source.expect("source present");
        if apply {
            if additive {
                let note = entry_of(&ctx, &to_key).map(|e| e.description).unwrap_or_default();
                let Some(value) = &value else {
                    return Err("An additive rename copies the value into a new secret, so the value must be supplied — the vault list deliberately carries ids and keys only, never values.".into());
                };
                writer.create(&to_key, value, &source.project_id, &note)?;
            } else {
                writer.rename(&source.id, &to_key)?;
            }
        }
        if additive {
            out(&format!(
                "{}  {from_key} -> {to_key}  (both names resolve; delete {from_key} after the code rename is verified)\n",
                if apply { "copied" } else { "would copy" }
            ));
        } else {
            out(&format!("{}  {from_key} -> {to_key}  (id preserved)\n", if apply { "renamed" } else { "would rename" }));
            out("NOTE: an in-place rename is a hard cutover. `sync-env` resolves a secret BY KEY, so if any\n      running deploy still references the old name, it breaks until the code rename lands.\n      Use --additive for those, then delete the old key once the new name is verified.\n");
        }
        if !apply {
            out("Dry run — re-run with --apply to write.\n");
        }
        Ok(0)
    })();
    match result {
        Ok(code) => Ok(code),
        Err(m) => Ok(plain(&m)),
    }
}
