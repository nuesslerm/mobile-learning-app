use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vm_ops_api::infrastructure::SystemClock;
use vm_ops_api_core::application::ports::Clock;

#[test]
fn given_the_system_clock_when_read_then_it_is_the_unix_time_in_milliseconds() {
    let before = SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis() as i64;

    let read = SystemClock.now_ms();

    let after = SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis() as i64;
    assert!(before <= read && read <= after, "{before} <= {read} <= {after}");
}

#[tokio::test]
async fn given_the_system_clock_when_it_sleeps_then_at_least_that_long_has_passed() {
    let started = Instant::now();

    SystemClock.sleep(Duration::from_millis(30)).await;

    assert!(started.elapsed() >= Duration::from_millis(30), "{:?}", started.elapsed());
}

#[tokio::test(start_paused = true)]
async fn given_a_paused_runtime_when_the_clock_sleeps_an_hour_then_exactly_an_hour_passes_on_the_runtimes_timer() {
    let clock: std::sync::Arc<dyn Clock> = std::sync::Arc::new(SystemClock);
    let started = tokio::time::Instant::now();

    clock.sleep(Duration::from_secs(3600)).await;

    assert_eq!(started.elapsed(), Duration::from_secs(3600));
}
