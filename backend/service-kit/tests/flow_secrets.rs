//! A deployed service's vault fallback: a registered variable its container was not given is asked of `wf secret env`.
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn given_a_registered_variable_the_container_lacks_when_secrets_load_then_wf_secret_env_fills_it() {
    let dir = std::env::temp_dir().join(format!("service-kit-secrets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (argv, stub) = (dir.join("argv"), dir.join("wf"));
    std::fs::write(&stub, format!("#!/bin/sh\necho \"$@\" > '{}'\necho 'GROQ_API_KEY=\"from-the-vault\"'\n", argv.display())).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Given every registered variable set but one, and wf answering for the vault
    for name in service_kit::secrets::env_vars() {
        std::env::set_var(name, "given");
    }
    std::env::remove_var("GROQ_API_KEY");
    std::env::set_var("WF_BIN", &stub);

    // When the service loads its secrets
    service_kit::secrets::load().await;

    // Then wf was asked for that one key with its `secret env` command, and the value is in the environment
    assert_eq!(std::fs::read_to_string(&argv).unwrap().trim(), "secret env --keys GROQ_API_KEY");
    assert_eq!(std::env::var("GROQ_API_KEY").as_deref(), Ok("from-the-vault"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn given_the_vault_registry_when_service_kit_embeds_its_copy_then_they_are_the_same_file() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../clis/secrets/src/data/registry.json");

    let copy = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/registry.json")).unwrap();

    assert_eq!(copy, std::fs::read_to_string(source).unwrap(), "copy clis/secrets/src/data/registry.json to backend/service-kit/data/registry.json and release service-kit");
}
