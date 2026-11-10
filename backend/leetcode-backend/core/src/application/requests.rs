//! What the use cases are asked to do. They derive `Deserialize` so the api layer can read them straight from a request body;
//! a field that must not be empty is a `NonEmpty`, so that rule fails at parse time like any other shape error.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// A string that is never empty.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub struct NonEmpty(String);

impl TryFrom<String> for NonEmpty {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err("must not be empty")
        } else {
            Ok(Self(value))
        }
    }
}

impl NonEmpty {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRequest {
    pub slug: NonEmpty,
    pub lang: NonEmpty,
    pub question_id: Number,
    pub code: String,
    pub data_input: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitRequest {
    pub slug: NonEmpty,
    pub lang: NonEmpty,
    pub question_id: Number,
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompareMode {
    Exact,
    Sorted,
    SetOfSorted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub inputs: Map<String, Value>,
    pub expected: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_mode: Option<CompareMode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub lang: NonEmpty,
    pub code: String,
    pub method_name: NonEmpty,
    pub test_cases: Vec<TestCase>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FormatRequest {
    pub lang: NonEmpty,
    pub code: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionRequest {
    pub cookie: NonEmpty,
}
