use vm_ops_api_core::application::ports::Ids;

/// Random version 4 UUIDs from the operating system's generator.
pub struct RandomIds;

impl Ids for RandomIds {
    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}
