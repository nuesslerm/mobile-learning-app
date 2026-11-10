//! `logger emit` (also `wf log emit`): one structured line from a shell script, written to the console (stderr for warn and
//! error) and sent to the OTLP ingest at once. The last stdout line is `{"sent":...}`; a line that could not be sent, for want
//! of a destination or because the ingest refused it, exits 1 with the reason on stderr.

use crate::oneshot::{self, Record};
use crate::redact::Redactor;
use crate::{Destination, Source};
use serde_json::json;

pub const LEVELS: [&str; 4] = ["info", "debug", "warn", "error"];
pub const USAGE: &str = "usage: logger emit --service <name> --message <event.name> [--level info|debug|warn|error] [--host-label <label>] [--attr key=value]...";
const NO_DESTINATION: &str = "no OTLP destination: BETTERSTACK_OTLP_TOKEN and BETTERSTACK_OTLP_HOST must both be set, the host a bare host name, and SECRETS_LOG_BETTERSTACK not false";

pub struct Emit {
    pub service: String,
    pub message: String,
    pub level: &'static str,
    pub host_label: Option<String>,
    pub attrs: Vec<(String, String)>,
}

pub fn level(name: &str) -> Result<&'static str, String> {
    LEVELS.into_iter().find(|l| *l == name).ok_or_else(|| format!("invalid --level \"{name}\", must be one of: {}", LEVELS.join(", ")))
}

/// The arguments after `emit`: `--flag value` or `--flag=value`, `--attr` repeatable.
pub fn parse(args: &[String]) -> Result<Emit, String> {
    let (mut service, mut message, mut level_name, mut host_label, mut attrs) = (None, None, "info".to_string(), None, Vec::new());
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let (flag, inline) = word.split_once('=').map_or((word.as_str(), None), |(f, v)| (f, Some(v.to_string())));
        let mut value = || inline.clone().or_else(|| words.next().cloned()).ok_or_else(|| format!("{flag} needs a value"));
        match flag {
            "--service" => service = Some(value()?),
            "--message" => message = Some(value()?),
            "--level" => level_name = value()?,
            "--host-label" => host_label = Some(value()?),
            "--attr" => {
                let pair = value()?;
                let (k, v) = pair.split_once('=').ok_or_else(|| format!("--attr \"{pair}\" is not key=value"))?;
                attrs.push((k.to_string(), v.to_string()));
            }
            other => return Err(format!("unknown argument \"{other}\"")),
        }
    }
    Ok(Emit {
        service: service.ok_or("--service is required")?,
        message: message.ok_or("--message is required")?,
        level: level(&level_name)?,
        host_label,
        attrs,
    })
}

/// Prints the line, sends it, prints `{"sent":...}`; 0 when sent, 1 otherwise.
pub fn run(emit: Emit) -> i32 {
    // A shell script has no version or call site of its own; `source` keeps the name queries filter on.
    let mut fields = vec![("source".to_string(), emit.service.clone())];
    fields.extend(emit.host_label.filter(|h| !h.is_empty()).map(|h| ("hostLabel".to_string(), h)));
    for (key, value) in emit.attrs {
        match fields.iter_mut().find(|(k, _)| *k == key) {
            Some(field) => field.1 = value,
            None => fields.push((key, value)),
        }
    }
    let record = Record { site: None, ..fields.into_iter().fold(Record::new(emit.level, emit.message), |r, (k, v)| r.attr(k, v)) };
    let line = record.line(&Redactor::from_env(Vec::new()));
    if emit.level == "warn" || emit.level == "error" {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    let sent = match Destination::from_env() {
        None => Err(NO_DESTINATION.to_string()),
        Some(destination) => oneshot::send(&destination, &Source::new(emit.service, ""), std::slice::from_ref(&record)).map_err(|why| format!("export failed: {why}")),
    };
    match sent {
        Ok(()) => {
            println!("{}", json!({ "sent": true }));
            0
        }
        Err(reason) => {
            println!("{}", json!({ "sent": false, "reason": reason }));
            eprintln!("[logger] emit failed: {reason}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn given_both_flag_spellings_and_repeated_attrs_when_parsed_then_every_value_is_kept_in_order() {
        let e = parse(&words("--service=svc --message evt.name --level warn --attr a=b --attr k=v=w")).unwrap();

        assert_eq!((e.service.as_str(), e.message.as_str(), e.level), ("svc", "evt.name", "warn"));
        assert_eq!(e.attrs, vec![("a".to_string(), "b".to_string()), ("k".to_string(), "v=w".to_string())]);
    }

    #[test]
    fn given_a_bad_command_line_when_parsed_then_it_names_what_is_wrong() {
        assert_eq!(parse(&words("--message m")).err().as_deref(), Some("--service is required"));
        assert_eq!(parse(&words("--service s --message m --level loud")).err().as_deref(), Some("invalid --level \"loud\", must be one of: info, debug, warn, error"));
        assert_eq!(parse(&words("--service s --message m --attr nope")).err().as_deref(), Some("--attr \"nope\" is not key=value"));
        assert_eq!(parse(&words("--service s --message m --colour red")).err().as_deref(), Some("unknown argument \"--colour\""));
    }
}
