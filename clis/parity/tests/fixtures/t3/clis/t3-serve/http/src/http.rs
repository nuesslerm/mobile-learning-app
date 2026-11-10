const THREADS: &str = "/api/orchestration/threads/";

pub fn handle(request: Request) {
    match (request.method.as_str(), request.path.as_str()) {
        ("OPTIONS", _) => respond(204),
        ("GET", path) if path.starts_with(THREADS) => {
            let answer = match path[THREADS.len()..].split_once('/') {
                Some((thread, "history")) => history(thread),
                _ => return respond(404),
            };
        }
        _ => respond(404),
    }
}
