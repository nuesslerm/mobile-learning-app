//! The doubles of everything the service reaches outside its process live in `remote-chat-agent-core`'s `testing` feature; the flow
//! tests name them here.

pub use remote_chat_agent_core::testing::{ev, fails, idle, says, CommandsPort, FakeCommands, FakeOpencode, FakeOutbound, OpencodePort, OutboundPort, Posted, Prompt, Run, Script};
