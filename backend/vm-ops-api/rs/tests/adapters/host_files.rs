use std::path::Path;
use std::sync::Arc;
use vm_ops_api::infrastructure::ProcHostFiles;
use vm_ops_api_core::application::ports::HostFiles;

const MEMINFO: &str = "MemTotal:        2097152 kB\nMemAvailable:    1048576 kB\nSwapTotal:        524288 kB\n";
const LOADAVG: &str = "0.25 0.50 0.75 1/100 5\n";

fn files_at(root: &Path) -> Arc<dyn HostFiles> {
    Arc::new(ProcHostFiles::new(root))
}

fn proc_tree(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).expect("a proc file");
    }
    dir
}

#[tokio::test]
async fn given_a_proc_tree_when_meminfo_and_loadavg_are_read_then_each_is_that_files_exact_text() {
    let tree = proc_tree(&[("meminfo", MEMINFO.as_bytes()), ("loadavg", LOADAVG.as_bytes())]);

    let files = files_at(tree.path());

    assert_eq!(files.read_text("meminfo").await.as_deref(), Some(MEMINFO));
    assert_eq!(files.read_text("loadavg").await.as_deref(), Some(LOADAVG));
}

#[tokio::test]
async fn given_no_such_file_when_read_then_it_is_absent_whether_the_tree_or_only_the_file_is_missing() {
    let tree = proc_tree(&[("meminfo", MEMINFO.as_bytes())]);

    assert_eq!(files_at(tree.path()).read_text("loadavg").await, None);
    assert_eq!(files_at(&tree.path().join("no-such-tree")).read_text("meminfo").await, None);
}

#[tokio::test]
async fn given_a_name_that_is_a_directory_when_read_then_it_is_absent_like_any_file_that_cannot_be_read() {
    let tree = proc_tree(&[]);
    std::fs::create_dir(tree.path().join("pressure")).expect("a dir");

    assert_eq!(files_at(tree.path()).read_text("pressure").await, None);
}

#[tokio::test]
async fn given_bytes_that_are_not_utf8_when_read_then_they_decode_as_nodes_readfile_does() {
    // Each expectation is what `fs.readFileSync(file, 'utf8')` returned for the same bytes on Node 22.
    let cases: [(&[u8], &str); 6] = [
        (b"MemTotal:\xff 1 kB\n", "MemTotal:\u{fffd} 1 kB\n"),
        (b"A\xe2\x82B", "A\u{fffd}B"),
        (b"\xef\xbb\xbfA", "\u{feff}A"),
        (b"\xf0\x9fA", "\u{fffd}A"),
        (b"\xed\xa0\x80A", "\u{fffd}\u{fffd}\u{fffd}A"),
        (b"\xc0\x80", "\u{fffd}\u{fffd}"),
    ];
    for (bytes, text) in cases {
        let tree = proc_tree(&[("meminfo", bytes)]);

        let read = files_at(tree.path()).read_text("meminfo").await;

        assert_eq!(read.as_deref(), Some(text), "bytes {bytes:?}");
    }
}

#[tokio::test]
async fn given_a_file_rewritten_between_two_reads_when_read_again_then_each_read_is_current() {
    let tree = proc_tree(&[("loadavg", b"0.10 0.20 0.30 1/50 7\n")]);
    let files = files_at(tree.path());

    let first = files.read_text("loadavg").await;
    std::fs::write(tree.path().join("loadavg"), LOADAVG).expect("a proc file");
    let second = files.read_text("loadavg").await;

    assert_eq!((first.as_deref(), second.as_deref()), (Some("0.10 0.20 0.30 1/50 7\n"), Some(LOADAVG)));
}

#[tokio::test]
async fn given_a_name_with_a_leading_slash_when_read_then_it_is_still_under_the_root() {
    let tree = proc_tree(&[("meminfo", MEMINFO.as_bytes())]);

    assert_eq!(files_at(tree.path()).read_text("/meminfo").await.as_deref(), Some(MEMINFO));
}

#[tokio::test]
async fn given_the_kernels_proc_when_meminfo_and_loadavg_are_read_then_they_are_its_text_though_their_size_is_zero() {
    let files = files_at(Path::new("/proc"));

    let meminfo = files.read_text("meminfo").await.expect("the kernel's meminfo");
    let loadavg = files.read_text("loadavg").await.expect("the kernel's loadavg");

    assert!(meminfo.starts_with("MemTotal:"), "{meminfo}");
    let load: Vec<f64> = loadavg.split(' ').take(3).map(|n| n.parse().expect("a load figure")).collect();
    assert_eq!(load.len(), 3, "{loadavg}");
}
