// Per-app deploy sets for `sync-env --for <app>`. `required` keys hard-fail the sync when one cannot be resolved; `optional`
// keys are delivered when present and skipped when absent from the vault, which in practice means telemetry only
// (BETTERSTACK_OTLP_HOST lives here, not in a committed .env.defaults: the host and its token are one credential pair, and
// splitting them across two delivery mechanisms is what let the host go missing while the token was fine). Model-provider
// keys are deliberately NOT optional: absent-and-skipped produced an agent that deployed clean and failed on first use. A
// broken backend still fails for optional keys too; the skip applies only to a genuine "not in the vault". The write-capable
// BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN appears in NO group, so no deploy can deliver it to a VM.
// `vm_only` keys of a group (a subset of its required/optional) are read where the app runs, never by a CI job: the runner's
// verify and `scope-check` skip them. The sets live in groups.json, which Terraform also reads so a fresh VM's cloud-init writes the same env before any wf exists.
pub struct Group {
    pub app: String,
    pub required: Vec<String>,
    pub optional: Vec<String>,
    pub vm_only: Vec<String>,
}

pub static GROUPS: std::sync::LazyLock<Vec<Group>> = std::sync::LazyLock::new(|| {
    let parsed: serde_json::Value = serde_json::from_str(include_str!("groups.json")).expect("groups.json is valid");
    let keys = |v: &serde_json::Value| v.as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
    parsed.as_array().expect("groups.json is a list").iter()
        .map(|g| Group { app: g["app"].as_str().expect("a group names its app").to_string(), required: keys(&g["required"]), optional: keys(&g["optional"]), vm_only: keys(&g["vmOnly"]) })
        .collect()
});
