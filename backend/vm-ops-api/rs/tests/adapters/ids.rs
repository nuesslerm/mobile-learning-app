use std::collections::HashSet;
use std::sync::Arc;
use vm_ops_api::infrastructure::RandomIds;
use vm_ops_api_core::application::ports::Ids;
use vm_ops_api_core::domain::validators::is_valid_job_id;

#[test]
fn given_the_random_ids_when_asked_then_each_is_a_lowercase_hyphenated_version_4_uuid_a_job_id_accepts() {
    let ids: Arc<dyn Ids> = Arc::new(RandomIds);

    for _ in 0..100 {
        let id = ids.uuid();

        let digits: Vec<char> = id.chars().collect();
        let shaped = digits.len() == 36
            && digits.iter().enumerate().all(|(at, c)| if matches!(at, 8 | 13 | 18 | 23) { *c == '-' } else { c.is_ascii_digit() || ('a'..='f').contains(c) });
        assert!(shaped, "{id}");
        assert_eq!(digits[14], '4', "{id}");
        assert!(matches!(digits[19], '8' | '9' | 'a' | 'b'), "{id}");
        assert!(is_valid_job_id(&id), "{id}");
    }
}

#[test]
fn given_a_thousand_ids_asked_when_compared_then_none_repeats() {
    let ids = RandomIds;

    let distinct: HashSet<String> = (0..1000).map(|_| ids.uuid()).collect();

    assert_eq!(distinct.len(), 1000);
}
