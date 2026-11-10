use vm_ops_api::infrastructure::binary_sha::{running_binary_sha, sha_of};

#[test]
fn given_a_file_when_hashed_then_the_first_12_hex_characters_of_its_sha256_are_returned() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let file = dir.path().join("abc");
    std::fs::write(&file, "abc").expect("a file");

    assert_eq!(sha_of(&file), "ba7816bf8f01");
}

#[test]
fn given_a_file_that_is_not_there_when_hashed_then_it_is_unknown() {
    let dir = tempfile::tempdir().expect("a temp dir");

    assert_eq!(sha_of(&dir.path().join("missing")), "unknown");
}

#[test]
fn given_the_running_test_binary_when_hashed_then_it_is_12_hex_characters() {
    let sha = running_binary_sha();

    assert!(sha.len() == 12 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{sha}");
}
