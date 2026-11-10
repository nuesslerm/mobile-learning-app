use libs_proxy_core::{route, Rejection, Route};

#[test]
fn given_a_sparse_index_path_when_routed_then_it_reads_the_matching_libs_index_object() {
    for (path, key) in [
        ("/index/1/a", "libs/index/1/a"),
        ("/index/2/ab", "libs/index/2/ab"),
        ("/index/3/s/svc", "libs/index/3/s/svc"),
        ("/index/se/rv/service-kit", "libs/index/se/rv/service-kit"),
    ] {
        assert_eq!(route("GET", path), Ok(Route::Index { key: key.into() }), "{path}");
    }
}

#[test]
fn given_a_download_path_when_routed_then_it_reads_the_crate_object_wf_lib_wrote() {
    assert_eq!(route("GET", "/dl/service-kit/1.2.3.crate"), Ok(Route::Crate { key: "libs/crates/service-kit/1.2.3.crate".into() }));
    assert_eq!(route("GET", "/dl/x/1.0.0-rc.1+b2.crate"), Ok(Route::Crate { key: "libs/crates/x/1.0.0-rc.1+b2.crate".into() }));
}

#[test]
fn given_config_and_health_when_routed_then_they_read_no_object() {
    assert_eq!(route("GET", "/index/config.json"), Ok(Route::Config));
    assert_eq!(route("GET", "/health"), Ok(Route::Health));
}

#[test]
fn given_a_traversal_or_malformed_path_when_routed_then_it_is_a_bad_request() {
    for path in [
        "/index/../config.json",
        "/index/se/rv/../../x",
        "/dl/../../etc/passwd.crate",
        "/index//se/rv/service-kit",
        "/index/se/rv/service-kit/",
        "/dl//1.0.0.crate",
        "/index/se%2Frv/service-kit",
        "/index/%2e%2e/config.json",
        "/dl/a%2Fb/1.0.0.crate",
        "/index/se\\rv/service-kit",
        "/index/./config.json",
        "index/config.json",
    ] {
        assert_eq!(route("GET", path), Err(Rejection::BadRequest), "{path}");
    }
}

#[test]
fn given_an_unserved_or_misshapen_route_when_routed_then_it_is_not_found() {
    for path in ["/", "/other", "/index", "/index/", "/index/xx/yy/service-kit", "/index/se/rv/Service-Kit", "/index/1/ab", "/dl/a/1.0.0", "/dl/a/v1.crate", "/dl/a/b/1.0.0.crate", "/healthz"] {
        let result = route("GET", path);
        assert!(matches!(result, Err(Rejection::NotFound) | Err(Rejection::BadRequest)), "{path}: {result:?}");
    }
    assert_eq!(route("GET", "/other"), Err(Rejection::NotFound));
    assert_eq!(route("GET", "/index/xx/yy/service-kit"), Err(Rejection::NotFound));
}

#[test]
fn given_any_verb_but_get_when_routed_then_it_is_method_not_allowed() {
    for method in ["POST", "PUT", "DELETE", "HEAD", "PATCH"] {
        assert_eq!(route(method, "/index/config.json"), Err(Rejection::MethodNotAllowed), "{method}");
        assert_eq!(route(method, "/dl/a/1.0.0.crate"), Err(Rejection::MethodNotAllowed), "{method}");
    }
}
