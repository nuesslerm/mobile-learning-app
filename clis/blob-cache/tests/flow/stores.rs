//! The store list of a role: resolved from config values, several stores at once, against the in-process PAR, S3 and bearer doubles.
use super::par_fake::{closed_url, ParFake, TOKEN};
use super::s3_fake::{BearerFake, S3Fake, ACCESS_KEY, BEARER, BUCKET, REGION, SECRET_KEY};
use blob_cache::domain::{Generation, Key, Kind};
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use blob_cache::stores::{self, ConfigError, BUILD_CACHE};
use blob_cache::Stores;
use cli_http::Via;
use std::collections::HashMap;

fn key(name: &str) -> Key {
    Key::new(Kind::Nx, Generation(0), name).unwrap()
}

fn s3_entry(fake: &S3Fake) -> String {
    format!("s3://{BUCKET}/ci/cache?endpoint={}&region={REGION}&path_style=1", fake.endpoint)
}

fn config(pairs: &[(&str, String)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn credentials() -> [(&'static str, String); 2] {
    [(stores::S3_READ_CREDENTIAL, format!("{ACCESS_KEY}:{SECRET_KEY}")), (stores::S3_WRITE_CREDENTIAL, format!("{ACCESS_KEY}:{SECRET_KEY}"))]
}

fn open(values: &HashMap<String, String>, write: bool) -> Result<Option<Stores>, ConfigError> {
    stores::open(&BUILD_CACHE, write, &|k| values.get(k).cloned(), Via::Direct)
}

/// Both planes name the same list, as one value each.
fn both(list: &str) -> Vec<(&'static str, String)> {
    vec![("BUILD_CACHE_READ_STORES", list.to_string()), ("BUILD_CACHE_WRITE_STORES", list.to_string())]
}

fn with_credentials(mut pairs: Vec<(&'static str, String)>) -> HashMap<String, String> {
    pairs.extend(credentials());
    config(&pairs)
}

#[test]
fn given_a_list_of_two_stores_when_written_and_read_then_writes_reach_both_and_reads_come_from_the_first_that_has_the_key() {
    // Given a PAR store and an S3 store declared in one list
    let (par, s3) = (ParFake::start(), S3Fake::start());
    let list = format!("par+{},{}", par.url, s3_entry(&s3));
    let values = with_credentials(both(&list));
    let stores = open(&values, true).unwrap().unwrap();

    // When a key is written through the list
    let wrote = stores.put(&key("both"), b"in both", false);

    // Then it is in each store, seen through a list of that store alone
    let alone = |entry: String| open(&with_credentials(both(&entry)), true).unwrap().unwrap();
    assert_eq!(wrote, Ok(PutOutcome::Stored));
    assert_eq!(alone(format!("par+{}", par.url)).get(&key("both")), Ok(Some(b"in both".to_vec())));
    assert_eq!(alone(s3_entry(&s3)).get(&key("both")), Ok(Some(b"in both".to_vec())));

    // And a key only the second store holds is read from the second, while a key the first holds never asks the second
    alone(s3_entry(&s3)).put(&key("second-only"), b"2", false).unwrap();
    let before = s3.seen().len();
    assert_eq!(stores.get(&key("second-only")), Ok(Some(b"2".to_vec())));
    assert_eq!(stores.get(&key("absent")), Ok(None));
    let asked = s3.seen().len();
    assert_eq!(stores.get(&key("both")), Ok(Some(b"in both".to_vec())));
    assert!(asked > before && s3.seen().len() == asked, "the second store answered a key the first holds");
    assert_eq!(stores.labels().len(), 2);
}

#[test]
fn given_a_declared_store_that_is_unreachable_when_read_or_written_then_the_error_names_it_and_the_others_are_not_a_fallback() {
    // Given a list whose first store is closed and whose second is a working bucket
    let s3 = S3Fake::start();
    let closed = closed_url();
    let list = format!("par+{closed},{}", s3_entry(&s3));
    let stores = open(&with_credentials(both(&list)), true).unwrap().unwrap();

    // When a key is read, then written
    let read = stores.get(&key("k"));
    let put = stores.put(&key("k"), b"v", false);

    // Then both fail as unreachable, naming the closed store without its link, and the read never fell back to the second store
    let named = closed.split("/p/").next().unwrap();
    for error in [read.unwrap_err(), put.unwrap_err()] {
        assert!(matches!(error, StoreError::Unreachable(_)), "{error}");
        assert!(error.to_string().contains(&format!("par+{named}")) && !error.to_string().contains(TOKEN), "{error}");
    }

    // And the write still filled the reachable store, so one store down does not leave the other empty
    let only = open(&with_credentials(both(&s3_entry(&s3))), true).unwrap().unwrap();
    assert_eq!(only.get(&key("k")), Ok(Some(b"v".to_vec())));
}

#[test]
fn given_a_disabled_entry_when_the_list_is_used_then_it_is_never_contacted() {
    // Given a list whose first entry is written with a leading dash
    let (par, s3) = (ParFake::start(), S3Fake::start());
    let list = format!("-par+{}, {}", par.url, s3_entry(&s3));
    let stores = open(&with_credentials(both(&list)), true).unwrap().unwrap();

    // When it is written and read
    stores.put(&key("k"), b"v", true).unwrap();
    let read = stores.get(&key("k"));
    let exists = stores.exists(&key("nothing"));

    // Then only the enabled store answered, and the disabled one saw no request
    assert_eq!((read, exists, stores.len()), (Ok(Some(b"v".to_vec())), Ok(false), 1));
    assert!(par.seen().is_empty(), "{:?}", par.seen());
}

#[test]
fn given_no_enabled_store_when_opened_then_none_is_declared_and_a_missing_list_is_the_old_missing_link() {
    // Given a list of disabled entries only
    let par = ParFake::start();
    let off = config(&both(&format!("-par+{}", par.url)));

    // When opened for reading and for writing
    let (read, write) = (open(&off, false), open(&off, true));

    // Then no store is declared (the same as WF_BLOB_CACHE=off) and nothing was contacted
    assert!(matches!((&read, &write), (Ok(None), Ok(None))), "{read:?} {write:?}");
    assert!(par.seen().is_empty());

    // And with neither list nor legacy key, the keys that would do are named
    match open(&HashMap::new(), true) {
        Err(ConfigError::Missing(keys)) => assert!(keys[0].contains("BUILD_CACHE_READ_PAR_URL") && keys[0].contains("BUILD_CACHE_READ_STORES"), "{keys:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn given_only_the_legacy_par_keys_when_opened_then_one_par_store_serves_as_before_and_the_list_wins_when_both_are_set() {
    // Given the two legacy keys of a PAR bucket
    let (legacy, listed) = (ParFake::start(), ParFake::start());
    let old = config(&[("BUILD_CACHE_READ_PAR_URL", legacy.url.clone()), ("BUILD_CACHE_WRITE_PAR_URL", legacy.url.clone())]);

    // When a key is written and read through them
    let stores = open(&old, true).unwrap().unwrap();
    let wrote = stores.put(&key("k"), b"v", true);

    // Then it is one store, it works, and read-only resolution cannot write
    assert_eq!((stores.len(), wrote, stores.get(&key("k"))), (1, Ok(PutOutcome::Stored), Ok(Some(b"v".to_vec()))));
    let reader = open(&old, false).unwrap().unwrap();
    assert!(matches!(reader.put(&key("x"), b"v", false), Err(StoreError::Rejected(_))));

    // And a store list takes precedence over the legacy keys, which are then not contacted
    let mut both_set = old.clone();
    both_set.extend(config(&both(&format!("par+{}", listed.url))));
    open(&both_set, true).unwrap().unwrap().put(&key("l"), b"v", false).unwrap();
    assert_eq!(legacy.seen().iter().filter(|s| s.method == "PUT").count(), 1);
    assert_eq!(listed.seen().iter().filter(|s| s.method == "PUT").count(), 1);
}

#[test]
fn given_a_bearer_store_and_a_directory_store_when_listed_then_each_works_and_a_wrong_token_never_leaks() {
    // Given a bearer server and a directory in one list
    let dav = BearerFake::start();
    let dir = std::env::temp_dir().join(format!("stores-file-{}", std::process::id()));
    let list = format!("dav+{},file://{}", dav.url, dir.display());
    let mut pairs = both(&list);
    pairs.extend([(stores::DAV_READ_TOKEN, BEARER.to_string()), (stores::DAV_WRITE_TOKEN, BEARER.to_string())]);
    let values = config(&pairs);

    // When a key is written and read
    let stores = open(&values, true).unwrap().unwrap();
    let wrote = stores.put(&key("k"), b"v", false);
    let on_disk = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
    let read = stores.get(&key("k"));
    let _ = std::fs::remove_dir_all(&dir);

    // Then both stores hold it, the bearer server saw the token on every request
    assert_eq!((wrote, read), (Ok(PutOutcome::Stored), Ok(Some(b"v".to_vec()))));
    assert!(on_disk > 0 && dav.authorizations.lock().unwrap().iter().all(|a| a.as_deref() == Some(&format!("Bearer {BEARER}"))));

    // And a wrong token is a credential error that does not repeat the token
    let mut wrong = values.clone();
    wrong.insert(stores::DAV_READ_TOKEN.into(), "WRONG-TOKEN-VALUE".into());
    let error = open(&wrong, false).unwrap().unwrap().get(&key("k")).unwrap_err();
    assert!(matches!(error, StoreError::Credential(_)) && !error.to_string().contains("WRONG-TOKEN-VALUE"), "{error}");
}

#[test]
fn given_s3_credentials_when_anything_fails_then_neither_the_key_pair_nor_a_link_is_in_the_error() {
    // Given an S3 store signed with the wrong secret
    let s3 = S3Fake::start();
    let mut values = with_credentials(both(&s3_entry(&s3)));
    values.insert(stores::S3_READ_CREDENTIAL.into(), format!("{ACCESS_KEY}:wrong/secret+KEY"));
    values.insert(stores::S3_WRITE_CREDENTIAL.into(), format!("{ACCESS_KEY}:wrong/secret+KEY"));

    // When it is read, written and checked
    let stores = open(&values, true).unwrap().unwrap();
    let errors = [stores.get(&key("k")).unwrap_err(), stores.put(&key("k"), b"v", false).unwrap_err()];

    // Then each is a credential error naming the store, with no secret, access key or response body in it
    for error in errors {
        let text = error.to_string();
        assert!(matches!(error, StoreError::Credential(_)), "{text}");
        assert!(text.contains(&format!("s3://{BUCKET}@127.0.0.1")), "{text}");
        for secret in [ACCESS_KEY, "wrong/secret+KEY", SECRET_KEY, "ci/cache"] {
            assert!(!text.contains(secret), "{text}");
        }
    }

    // And a credential written into the URL is refused without repeating it, as are a malformed pair and a missing pair
    let in_url = open(&with_credentials(both(&format!("s3://{ACCESS_KEY}:{SECRET_KEY}@{BUCKET}/p?endpoint=https://s3.example"))), true).err().unwrap().to_string();
    let bad_pair = open(&config(&[("BUILD_CACHE_READ_STORES", s3_entry(&s3)), (stores::S3_READ_CREDENTIAL, "nocolonhere".into())]), false).err().unwrap().to_string();
    for text in [&in_url, &bad_pair] {
        assert!(!text.contains(SECRET_KEY) && !text.contains(ACCESS_KEY) && !text.contains("nocolonhere"), "{text}");
    }
    let absent = open(&config(&[("BUILD_CACHE_READ_STORES", s3_entry(&s3))]), false).err().unwrap();
    assert_eq!(absent, ConfigError::Missing(vec![stores::S3_READ_CREDENTIAL.to_string()]));
}

#[test]
fn given_write_and_read_lists_that_do_not_line_up_when_opened_for_writing_then_it_is_a_config_error() {
    // Given a read list of two stores and a write list of one
    let (a, b) = (ParFake::start(), ParFake::start());
    let values = config(&[("BUILD_CACHE_READ_STORES", format!("par+{},par+{}", a.url, b.url)), ("BUILD_CACHE_WRITE_STORES", format!("par+{}", a.url))]);

    // When opened for reading, then for writing
    let read = open(&values, false);
    let write = open(&values, true);

    // Then reading is fine and writing is refused, naming the rule rather than any link
    assert_eq!(read.unwrap().unwrap().len(), 2);
    let text = write.err().unwrap().to_string();
    assert!(text.contains("line up") && !text.contains(TOKEN), "{text}");
}
