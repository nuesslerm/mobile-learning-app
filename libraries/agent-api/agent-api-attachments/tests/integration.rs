//! The loader's masks (`agent_api_proto::testing::Mask`, applied in process by every contract test) against `wf mimic record` under the
//! same rules as a `pointer` rule set: the two must write the same value for every response of this crate's fixtures.
use agent_api_attachments::testing::fixtures;
use agent_api_proto::testing::mimic::assert_masks_match_wf_mimic;

#[test]
fn given_every_response_of_the_fixtures_when_masked_in_process_and_by_wf_mimic_then_the_values_are_identical() {
    assert_masks_match_wf_mimic(&fixtures());
}
