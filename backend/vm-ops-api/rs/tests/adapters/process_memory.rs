use vm_ops_api::infrastructure::OsProcessMemory;
use vm_ops_api_core::application::ports::ProcessMemory;

#[test]
fn given_a_running_process_when_its_memory_is_read_then_it_is_a_plausible_whole_number_of_megabytes() {
    let mb = OsProcessMemory.rss_mb();

    assert!((1..16_384).contains(&mb), "{mb} MB");
}
