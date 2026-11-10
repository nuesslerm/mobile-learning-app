//! The drivers' models and modes: the one place that knows them. A driver whose models the CLI decides (opencode) lists one entry that
//! stands for the CLI's own default.
use agent_api_catalog::{Choice, Model, ModelsResult};

fn choice(value: &str, label: &str, description: &str) -> Choice {
    Choice { value: value.into(), label: label.into(), description: Some(description.into()) }
}

fn model(id: &str, label: &str, description: Option<&str>, is_default: bool) -> Model {
    Model { id: id.into(), label: label.into(), description: description.map(str::to_string), is_default, default_effort: None, efforts: Vec::new() }
}

fn models(driver: &str) -> Vec<Model> {
    match driver {
        "claudeAgent" => vec![
            model("sonnet", "Sonnet", Some("Fast and capable for most work"), true),
            model("opus", "Opus", Some("The most capable"), false),
            model("haiku", "Haiku", Some("The fastest"), false),
        ],
        "codex" => vec![model("gpt-5-codex", "GPT-5 Codex", None, true), model("gpt-5", "GPT-5", None, false)],
        _ => vec![model("default", "Default", Some("The model opencode is configured with"), true)],
    }
}

/// The modes and models `turn.start` takes for `driver` (one of `agent_api_catalog::DRIVERS`).
pub fn for_driver(driver: &str) -> ModelsResult {
    ModelsResult {
        driver: driver.into(),
        models: models(driver),
        runtime_modes: vec![
            choice("approval-required", "Ask before changes", "Every edit and command waits for approval"),
            choice("auto-accept-edits", "Accept edits", "Edits go through, commands wait for approval"),
            choice("full-access", "Full access", "Edits and commands run without asking"),
        ],
        default_runtime_mode: "full-access".into(),
        interaction_modes: vec![choice("default", "Build", "Make the changes asked for"), choice("plan", "Plan", "Read only: propose a plan, change nothing")],
        default_interaction_mode: "default".into(),
    }
}

/// The executable each driver runs.
pub fn program(driver: &str) -> &'static str {
    match driver {
        "claudeAgent" => "claude",
        "codex" => "codex",
        _ => "opencode",
    }
}
