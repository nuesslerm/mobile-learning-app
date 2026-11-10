use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use telemetry::{oneshot::Record, record};

pub fn top_level_function() -> Record {
    record!("info", "top-level-function") // @site top-level-function fn=top_level_function
}

pub struct Greeter;

impl Greeter {
    pub fn greet(&self) -> Record {
        record!("info", "method") // @site method fn=greet
    }
}

pub fn outer() -> Record {
    fn inner() -> Record {
        record!("info", "nested-function") // @site nested-function fn=inner
    }
    inner()
}

pub fn with_callback() -> Vec<Record> {
    (0..1).map(|_| record!("info", "callback-in-function")).collect() // @site callback-in-function fn=with_callback
}

pub async fn async_function() -> Record {
    record!("info", "async-function") // @site async-function fn=async_function
}

/// Every record of the cases above, as the console lines the logger prints.
pub fn console_lines() -> Vec<String> {
    let redactor = telemetry::redact::Redactor::from_env(Vec::new());
    let mut records = vec![top_level_function(), Greeter.greet(), outer()];
    records.extend(with_callback());
    let mut future = pin!(async_function());
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(record) => records.push(record),
        Poll::Pending => panic!("the fixture's async function awaits nothing, so it completes on the first poll"),
    }
    records.iter().map(|record| record.line(&redactor)).collect()
}
