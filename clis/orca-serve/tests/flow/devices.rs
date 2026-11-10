//! The paired-devices file under concurrent writers: no registration is lost, whether the writers are instances in one process or processes.
use orca_crypto::ports::{Device, DeviceRegistry};
use orca_serve::adapters::FileDevices;
use serde_json::Value;
use std::process::{Command, Stdio};

const WRITERS: usize = 12;

fn ids(file: &std::path::Path) -> Vec<String> {
    let rows: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let mut ids: Vec<String> = rows.as_array().unwrap().iter().map(|d| d["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    ids
}

#[test]
fn given_two_instances_on_one_directory_when_devices_register_and_revoke_at_once_then_none_is_lost() {
    // Given one devices file and a separate registry instance per writer thread
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("devices.json");
    let wanted: Vec<String> = (0..WRITERS).map(|n| format!("dev-{n:02}")).collect();
    // When every writer registers its own device at once, and one of them then revokes another's
    let handles: Vec<_> = wanted.iter().cloned().map(|id| {
        let registry = FileDevices::new(file.clone());
        std::thread::spawn(move || registry.register(Device { token: format!("tok-{id}"), id }).unwrap())
    }).collect();
    handles.into_iter().for_each(|h| h.join().unwrap());
    // Then every device is held, and a revoke through a fresh instance removes exactly one
    assert_eq!(ids(&file), wanted);
    assert!(FileDevices::new(file.clone()).revoke("dev-03").unwrap());
    assert_eq!(ids(&file).len(), WRITERS - 1);
}

#[test]
fn given_processes_pairing_at_once_when_each_adds_a_device_then_the_file_holds_every_one() {
    // Given one state directory and the host key written by a first pairing
    let dir = tempfile::tempdir().unwrap();
    let pair = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_orca-serve"));
        c.env_remove("ORCA_RCA_BEARER").env_remove("ORCA_RCA_URL").env_remove("ORCA_STATE_DIR");
        c.args(["pair", "--endpoint", "ws://phone.example:6768", "--state-dir"]).arg(dir.path()).stdout(Stdio::null()).stderr(Stdio::null());
        c
    };
    assert!(pair().status().unwrap().success());
    // When WRITERS more processes pair at the same moment
    let children: Vec<_> = (0..WRITERS).map(|_| pair().spawn().unwrap()).collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    // Then the file holds one device per run, the first included
    assert_eq!(ids(&dir.path().join("devices.json")).len(), WRITERS + 1);
}
