//! A stand-in for a Better Stack OTLP metrics source: the gauges a `wf` run sent to a `MockServer`, read back from its requests.
use crate::MockRequest;
use serde_json::Value;

/// One gauge point as the source received it: the metric and its two labels.
#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    pub metric: String,
    pub scope: String,
    pub name: String,
    pub value: f64,
}

/// Every gauge of every `POST /v1/metrics` the double received, in arrival order.
pub fn gauges(requests: &[MockRequest]) -> Vec<Gauge> {
    let label = |point: &Value, key: &str| {
        point["attributes"].as_array().into_iter().flatten().find(|a| a["key"] == key).and_then(|a| a["value"]["stringValue"].as_str()).unwrap_or_default().to_string()
    };
    let number = |point: &Value| point["asDouble"].as_f64().or_else(|| point["asInt"].as_str().and_then(|n| n.parse().ok())).or_else(|| point["asInt"].as_f64());
    let mut seen = Vec::new();
    for request in requests.iter().filter(|r| r.method == "POST" && r.url.ends_with("/v1/metrics")) {
        let Ok(body) = serde_json::from_str::<Value>(&request.body) else { continue };
        for resource in body["resourceMetrics"].as_array().into_iter().flatten() {
            for scope in resource["scopeMetrics"].as_array().into_iter().flatten() {
                for metric in scope["metrics"].as_array().into_iter().flatten() {
                    for point in metric["gauge"]["dataPoints"].as_array().into_iter().flatten() {
                        if let (Some(name), Some(value)) = (metric["name"].as_str(), number(point)) {
                            seen.push(Gauge { metric: name.to_string(), scope: label(point, "scope"), name: label(point, "name"), value });
                        }
                    }
                }
            }
        }
    }
    seen
}
