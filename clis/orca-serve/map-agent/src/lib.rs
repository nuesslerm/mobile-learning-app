//! Orca's agent-session rows over rca: `agentSession.*` (create, send, cancel, history, item, subscribe, hold, release, currentRequests,
//! conversationCommand), `agent.launch`, `agent.launchReplay` and `nativeChat.*`. An Orca chat session is an rca thread (the session id is the
//! thread id), a send is a turn, a cancel a forced stop (it holds the queue), and the phone's journal is the thread's events folded (`fold`); the journal cursor is
//! the thread's `seq`. Nothing here touches a disk or a process.
mod agent;
mod attach;
mod catalog;
mod fold;
mod ids;
mod journal;
mod launch;
mod native_stream;
mod ops;
mod queued;
mod skills;
mod respond;
mod rewind;
mod session;
mod status_stream;
mod stream;
mod vault;
mod clipboard;
mod speech;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

use agent_api_agent::{queue, requests, THREAD_CREATE, THREAD_GET, THREAD_HISTORY, THREAD_LIST, TURN_START};
use agent_api_agent::lifecycle::{THREAD_FORCE_STOP, THREAD_ROLLBACK};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{self, string_field, Failure};
use orca_core::{Area, Ctx, Kind, Method, NotOffered, Subscription};
use serde_json::{json, Value};

pub use attach::max_bytes;
pub use catalog::drivers_served;
pub use session::{agent_of, driver_of};

pub const AGENT_SESSION_CREATE: &str = "agentSession.create";
pub const AGENT_SESSION_SEND: &str = "agentSession.send";
pub const AGENT_SESSION_CANCEL: &str = "agentSession.cancel";
pub const AGENT_SESSION_HISTORY: &str = "agentSession.history";
pub const AGENT_SESSION_ITEM: &str = "agentSession.item";
pub const AGENT_SESSION_SUBSCRIBE: &str = "agentSession.subscribe";
pub const AGENT_SESSION_UNSUBSCRIBE: &str = "agentSession.unsubscribe";
pub const AGENT_SESSION_HOLD: &str = "agentSession.hold";
pub const AGENT_SESSION_REVEAL: &str = "agentSession.reveal";
pub const AGENT_SESSION_RELEASE: &str = "agentSession.release";
pub const AGENT_SESSION_CURRENT_REQUESTS: &str = "agentSession.currentRequests";
pub const AGENT_SESSION_SEND_RECORDED: &str = "agentSession.sendRecorded";
pub const AGENT_SESSION_REWIND: &str = "agentSession.rewind";
pub const AGENT_SESSION_SUBSCRIBE_STATUS: &str = "agentSession.subscribeStatus";
pub const AGENT_SESSION_CONVERSATION_COMMAND: &str = "agentSession.conversationCommand";
pub const AGENT_SESSION_QUEUED_MESSAGE_DELETE: &str = "agentSession.queuedMessageDelete";
pub const AGENT_SESSION_QUEUED_MESSAGE_EDIT_HOLD: &str = "agentSession.queuedMessageEditHold";
pub const AGENT_SESSION_QUEUED_MESSAGE_MOVE: &str = "agentSession.queuedMessageMove";
pub const AGENT_SESSION_QUEUED_MESSAGE_SEND: &str = "agentSession.queuedMessageSend";
pub const AGENT_SESSION_QUEUED_MESSAGE_UPDATE: &str = "agentSession.queuedMessageUpdate";
pub const AGENT_SESSION_QUEUED_MESSAGES_RESUME: &str = "agentSession.queuedMessagesResume";
pub const AGENT_SESSION_RESPOND_TO_APPROVAL: &str = "agentSession.respondToApproval";
pub const AGENT_SESSION_RESPOND_TO_QUESTION: &str = "agentSession.respondToQuestion";
pub const AGENT_SESSION_ATTACHMENT_UPLOAD_START: &str = "agentSessionAttachment.uploadStart";
pub const AGENT_SESSION_ATTACHMENT_UPLOAD_APPEND: &str = "agentSessionAttachment.uploadAppend";
pub const AGENT_SESSION_ATTACHMENT_UPLOAD_COMMIT: &str = "agentSessionAttachment.uploadCommit";
pub const AGENT_SESSION_ATTACHMENT_UPLOAD_ABORT: &str = "agentSessionAttachment.uploadAbort";
pub const AGENT_SESSION_ATTACHMENT_READ: &str = "agentSessionAttachment.read";
pub const AGENT_SESSION_ATTACHMENT_READ_TEXT_CHUNK: &str = "agentSessionAttachment.readTextChunk";
pub const FILES_READ_CHAT_IMAGE: &str = "files.readChatImage";
pub const AGENT_SESSION_COMMANDS: &str = "agentSession.commands";
pub const AGENT_SESSION_CREATE_SUPPORT: &str = "agentSession.createSupport";
pub const AGENT_SESSION_MODEL_CATALOG: &str = "agentSession.modelCatalog";
pub const AGENT_SESSION_OPTIONS: &str = "agentSession.options";
pub const AGENT_SESSION_AGENTS: &str = "agentSession.agents";
pub const AGENT_SESSION_SET_OPTION: &str = "agentSession.setOption";
pub const PREFLIGHT_CHECK: &str = "preflight.check";
pub const SPEECH_MODELS_LIST: &str = "speech.models.list";
pub const SPEECH_MODELS_DOWNLOAD: &str = "speech.models.download";
pub const SPEECH_MODELS_DELETE: &str = "speech.models.delete";
pub const SPEECH_DICTATION_SETUP: &str = "speech.dictation.setup";
pub const SPEECH_DICTATION_START: &str = "speech.dictation.start";
pub const SPEECH_DICTATION_CHUNK: &str = "speech.dictation.chunk";
pub const SPEECH_DICTATION_FINISH: &str = "speech.dictation.finish";
pub const SPEECH_DICTATION_CANCEL: &str = "speech.dictation.cancel";
pub const CLIPBOARD_SAVE_IMAGE: &str = "clipboard.saveImageAsTempFile";
pub const CLIPBOARD_START_UPLOAD: &str = "clipboard.startImageUpload";
pub const CLIPBOARD_APPEND_CHUNK: &str = "clipboard.appendImageUploadChunk";
pub const CLIPBOARD_COMMIT_UPLOAD: &str = "clipboard.commitImageUpload";
pub const CLIPBOARD_ABORT_UPLOAD: &str = "clipboard.abortImageUpload";
pub const PREFLIGHT_DETECT_AGENTS: &str = "preflight.detectAgents";
pub const PREFLIGHT_DETECT_REMOTE_AGENTS: &str = "preflight.detectRemoteAgents";
pub const PREFLIGHT_REFRESH_AGENTS: &str = "preflight.refreshAgents";
pub const SKILLS_DISCOVER: &str = "skills.discover";
pub const AI_VAULT_LIST_SESSIONS: &str = "aiVault.listSessions";
pub const AI_VAULT_PREPARE_SESSION_RESUME: &str = "aiVault.prepareSessionResume";
pub const AGENT_LAUNCH: &str = "agent.launch";
pub const AGENT_LAUNCH_REPLAY: &str = "agent.launchReplay";
pub const NATIVE_CHAT_READ_SESSION: &str = "nativeChat.readSession";
pub const NATIVE_CHAT_UNSUBSCRIBE: &str = "nativeChat.unsubscribe";
pub const NATIVE_CHAT_SUBSCRIBE: &str = "nativeChat.subscribe";

/// The agent methods this crate sends: its calls read this list, and a flow test holds it to the methods really sent.
pub const USES: &[AgentMethod] = &[THREAD_LIST, THREAD_CREATE, THREAD_GET, THREAD_HISTORY, TURN_START, THREAD_FORCE_STOP, THREAD_ROLLBACK, agent_api_catalog::SESSIONS_LIST, queue::QUEUE_ADD, queue::QUEUE_LIST, queue::QUEUE_EDIT, queue::QUEUE_MOVE, queue::QUEUE_DELETE, queue::QUEUE_RESUME, queue::TURN_STEER, agent_api_attachments::UPLOAD_PUT, agent_api_attachments::GET, agent_api_attachments::STAGE, requests::APPROVAL_ANSWER, requests::INPUT_ANSWER, requests::INPUT_DISMISS, agent_api_state::SET, agent_api_catalog::MODELS_LIST, agent_api_catalog::AGENTS_DETECT, agent_api_catalog::SKILLS_LIST, agent_api_catalog::COMMANDS_LIST, agent_api_catalog::SPEECH_STATUS, agent_api_catalog::SPEECH_TRANSCRIBE, orca_map_hosted::AUTH_DISCOVER];

const METHODS: &[Method] = &[
    Method { name: AGENT_SESSION_CREATE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_SEND, kind: Kind::Unary },
    Method { name: AGENT_SESSION_CANCEL, kind: Kind::Unary },
    Method { name: AGENT_SESSION_HISTORY, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ITEM, kind: Kind::Unary },
    Method { name: AGENT_SESSION_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: NATIVE_CHAT_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: AGENT_SESSION_SUBSCRIBE_STATUS, kind: Kind::Streaming },
    Method { name: AGENT_SESSION_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_HOLD, kind: Kind::Unary },
    Method { name: AGENT_SESSION_REVEAL, kind: Kind::Unary },
    Method { name: AGENT_SESSION_RELEASE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_CURRENT_REQUESTS, kind: Kind::Unary },
    Method { name: AGENT_SESSION_SEND_RECORDED, kind: Kind::Unary },
    Method { name: AGENT_SESSION_REWIND, kind: Kind::Unary },
    Method { name: AGENT_SESSION_CONVERSATION_COMMAND, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGE_DELETE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGE_EDIT_HOLD, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGE_MOVE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGE_SEND, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGE_UPDATE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_QUEUED_MESSAGES_RESUME, kind: Kind::Unary },
    Method { name: AGENT_SESSION_RESPOND_TO_APPROVAL, kind: Kind::Unary },
    Method { name: AGENT_SESSION_RESPOND_TO_QUESTION, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_UPLOAD_START, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_UPLOAD_APPEND, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_UPLOAD_COMMIT, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_UPLOAD_ABORT, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_READ, kind: Kind::Unary },
    Method { name: AGENT_SESSION_ATTACHMENT_READ_TEXT_CHUNK, kind: Kind::Unary },
    Method { name: FILES_READ_CHAT_IMAGE, kind: Kind::Unary },
    Method { name: AGENT_SESSION_COMMANDS, kind: Kind::Unary },
    Method { name: AGENT_SESSION_CREATE_SUPPORT, kind: Kind::Unary },
    Method { name: AGENT_SESSION_MODEL_CATALOG, kind: Kind::Unary },
    Method { name: AGENT_SESSION_OPTIONS, kind: Kind::Unary },
    Method { name: AGENT_SESSION_AGENTS, kind: Kind::Unary },
    Method { name: AGENT_SESSION_SET_OPTION, kind: Kind::Unary },
    Method { name: PREFLIGHT_CHECK, kind: Kind::Unary },
    Method { name: SPEECH_MODELS_LIST, kind: Kind::Unary },
    Method { name: SPEECH_MODELS_DOWNLOAD, kind: Kind::Unary },
    Method { name: SPEECH_MODELS_DELETE, kind: Kind::Unary },
    Method { name: SPEECH_DICTATION_SETUP, kind: Kind::Unary },
    Method { name: SPEECH_DICTATION_START, kind: Kind::Unary },
    Method { name: SPEECH_DICTATION_CHUNK, kind: Kind::Unary },
    Method { name: SPEECH_DICTATION_FINISH, kind: Kind::Unary },
    Method { name: SPEECH_DICTATION_CANCEL, kind: Kind::Unary },
    Method { name: CLIPBOARD_SAVE_IMAGE, kind: Kind::Unary },
    Method { name: CLIPBOARD_START_UPLOAD, kind: Kind::Unary },
    Method { name: CLIPBOARD_APPEND_CHUNK, kind: Kind::Unary },
    Method { name: CLIPBOARD_COMMIT_UPLOAD, kind: Kind::Unary },
    Method { name: CLIPBOARD_ABORT_UPLOAD, kind: Kind::Unary },
    Method { name: PREFLIGHT_DETECT_AGENTS, kind: Kind::Unary },
    Method { name: PREFLIGHT_DETECT_REMOTE_AGENTS, kind: Kind::Unary },
    Method { name: PREFLIGHT_REFRESH_AGENTS, kind: Kind::Unary },
    Method { name: SKILLS_DISCOVER, kind: Kind::Unary },
    Method { name: AI_VAULT_LIST_SESSIONS, kind: Kind::Unary },
    Method { name: AI_VAULT_PREPARE_SESSION_RESUME, kind: Kind::Unary },
    Method { name: AGENT_LAUNCH, kind: Kind::Unary },
    Method { name: AGENT_LAUNCH_REPLAY, kind: Kind::Unary },
    Method { name: NATIVE_CHAT_READ_SESSION, kind: Kind::Unary },
    Method { name: NATIVE_CHAT_UNSUBSCRIBE, kind: Kind::Unary },
];

/// Each reason equals the one in `docs/plans/rca-parity/coverage/orca-decisions.json`; a flow test compares them.
pub const NOT_OFFERED: &[NotOffered] = &[
    NotOffered { name: "orchestration.partyLocation", reason: "owner-confirmed (decision 7): it resolves a party address in the desktop's orchestration database, which this host does not keep; rca has no orchestration runs or terminal workers to locate", decided_by: "M7" },
    NotOffered { name: "orchestration.partyOpen", reason: "owner-confirmed (decision 7): it opens a party of the desktop's orchestration database in a desktop tab or terminal, and this host has neither a desktop nor that database", decided_by: "M7" },
    NotOffered { name: "orchestration.workerAutoClose", reason: "owner-confirmed (decision 7): it flips the desktop setting that closes a terminal worker's tab when its work ends; this host has no desktop terminal workers", decided_by: "M7" },
    NotOffered { name: "orchestration.workerKeepOpen", reason: "owner-confirmed (decision 7): it keeps a desktop terminal worker's tab open after its work ends; this host has no desktop terminal workers", decided_by: "M7" },
    NotOffered { name: "agentSession.readVisual", reason: "owner-confirmed (decision 7): it reads a screenshot folder under the desktop's user data that the desktop's own chat view fills; rca keeps no such folder per chat, so any answer would pass by refusing or by an empty read", decided_by: "M7" },
];

/// The hash the phone compares a queued card's body by (the recorded Orca fingerprint of a one-block user message).
pub fn body_fingerprint(session: &str, text: &str) -> String {
    queued::fingerprint(session, &queued::body_of(text))
}

/// The agent rows; it holds the uploads in flight, so one value serves every connection.
#[derive(Default)]
pub struct AgentArea {
    uploads: attach::Uploads,
    clips: clipboard::Clips,
    dictations: speech::Dictations,
}

impl Area for AgentArea {
    fn name(&self) -> &'static str {
        "agent"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn not_offered(&self) -> &'static [NotOffered] {
        NOT_OFFERED
    }

    fn capabilities(&self) -> &'static [&'static str] {
        &[status_stream::STRUCTURED, status_stream::STATUS_FEED]
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            AGENT_SESSION_CREATE => session::create(agent, params),
            AGENT_SESSION_SEND => session::send(agent, params),
            AGENT_SESSION_CANCEL => session::cancel(agent, params),
            AGENT_SESSION_HISTORY => session::history(agent, params),
            AGENT_SESSION_ITEM => session::item(agent, params),
            AGENT_SESSION_UNSUBSCRIBE => unsubscribe(ctx, params),
            AGENT_SESSION_HOLD => session::hold(agent, params),
            AGENT_SESSION_REVEAL => session::reveal(agent, params),
            AGENT_SESSION_RELEASE => session::release(params),
            AGENT_SESSION_CURRENT_REQUESTS => session::current_requests(agent, params),
            AGENT_SESSION_SEND_RECORDED => session::send_recorded(agent, params),
            AGENT_SESSION_REWIND => rewind::rewind(agent, params),
            AGENT_SESSION_CONVERSATION_COMMAND => session::conversation_command(agent, params),
            AGENT_SESSION_QUEUED_MESSAGE_DELETE => queued::delete(agent, params),
            AGENT_SESSION_QUEUED_MESSAGE_EDIT_HOLD => queued::edit_hold(agent, params),
            AGENT_SESSION_QUEUED_MESSAGE_MOVE => queued::move_to(agent, params),
            AGENT_SESSION_QUEUED_MESSAGE_SEND => queued::send(agent, params),
            AGENT_SESSION_QUEUED_MESSAGE_UPDATE => queued::update(agent, params),
            AGENT_SESSION_QUEUED_MESSAGES_RESUME => queued::resume(agent, params),
            AGENT_SESSION_RESPOND_TO_APPROVAL => respond::respond(agent, params, "approval"),
            AGENT_SESSION_RESPOND_TO_QUESTION => respond::respond(agent, params, "question"),
            AGENT_SESSION_ATTACHMENT_UPLOAD_START => attach::start(&self.uploads, ctx.connection_id, agent, params),
            AGENT_SESSION_ATTACHMENT_UPLOAD_APPEND => attach::append(&self.uploads, ctx.connection_id, params),
            AGENT_SESSION_ATTACHMENT_UPLOAD_COMMIT => attach::commit(&self.uploads, ctx.connection_id, agent, params),
            AGENT_SESSION_ATTACHMENT_UPLOAD_ABORT => attach::abort(&self.uploads, ctx.connection_id, params),
            AGENT_SESSION_ATTACHMENT_READ => attach::read(agent, params),
            AGENT_SESSION_ATTACHMENT_READ_TEXT_CHUNK => attach::read_text_chunk(agent, params),
            FILES_READ_CHAT_IMAGE => attach::read_chat_image(agent, params),
            AGENT_SESSION_COMMANDS => catalog::commands(agent, params),
            AGENT_SESSION_CREATE_SUPPORT => catalog::create_support(agent, params),
            AGENT_SESSION_MODEL_CATALOG => catalog::model_catalog(agent, params),
            AGENT_SESSION_OPTIONS => catalog::options(agent, params),
            AGENT_SESSION_AGENTS => catalog::agents(params),
            AGENT_SESSION_SET_OPTION => catalog::set_option(agent, params),
            PREFLIGHT_CHECK => catalog::preflight_check(agent, params),
            SPEECH_MODELS_LIST => speech::list(agent, params),
            SPEECH_MODELS_DOWNLOAD => speech::download(agent, params),
            SPEECH_MODELS_DELETE => speech::delete(params),
            SPEECH_DICTATION_SETUP => speech::configure(agent, params),
            SPEECH_DICTATION_START => speech::start(&self.dictations, ctx.device_id, agent, params),
            SPEECH_DICTATION_CHUNK => speech::chunk(&self.dictations, ctx.device_id, params),
            SPEECH_DICTATION_FINISH => speech::finish(&self.dictations, ctx.device_id, agent, params),
            SPEECH_DICTATION_CANCEL => speech::cancel(&self.dictations, ctx.device_id, params),
            CLIPBOARD_SAVE_IMAGE => clipboard::save(agent, params),
            CLIPBOARD_START_UPLOAD => clipboard::start(&self.clips, ctx.device_id, params),
            CLIPBOARD_APPEND_CHUNK => clipboard::append(&self.clips, ctx.device_id, params),
            CLIPBOARD_COMMIT_UPLOAD => clipboard::commit(&self.clips, ctx.device_id, agent, params),
            CLIPBOARD_ABORT_UPLOAD => clipboard::abort(&self.clips, ctx.device_id, params),
            PREFLIGHT_DETECT_AGENTS => catalog::detect_agents(agent, params),
            PREFLIGHT_DETECT_REMOTE_AGENTS => catalog::detect_remote_agents(agent, params),
            PREFLIGHT_REFRESH_AGENTS => catalog::refresh_agents(agent, params),
            SKILLS_DISCOVER => skills::discover(agent, params),
            AI_VAULT_LIST_SESSIONS => vault::list_sessions(agent, params),
            AI_VAULT_PREPARE_SESSION_RESUME => vault::prepare_resume(params),
            AGENT_LAUNCH => launch::launch(agent, params),
            AGENT_LAUNCH_REPLAY => launch::replay(agent, params),
            NATIVE_CHAT_READ_SESSION => session::read_session(agent, params),
            // With a subscription id, that one native-chat stream of this connection; without, all of them.
            NATIVE_CHAT_UNSUBSCRIBE => {
                let base = native_stream::prefix(ctx.connection_id);
                match string_field(params, "subscriptionId")? {
                    Some("") => return Err(Failure::invalid("Too small: expected string to have >=1 characters")),
                    Some(id) => ctx.subscriptions.cancel(&format!("{base}:{id}")),
                    None => ctx.subscriptions.cancel_prefix(&base) > 0,
                };
                Ok(json!({"unsubscribed": true}))
            }
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }

    fn open(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        match method {
            AGENT_SESSION_SUBSCRIBE => stream::open(ctx, params),
            NATIVE_CHAT_SUBSCRIBE => native_stream::open(ctx, params),
            AGENT_SESSION_SUBSCRIBE_STATUS => status_stream::open(ctx),
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

/// With a subscription id, that one stream of the session; without, every stream this connection has on it.
fn unsubscribe(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let session = session::agent_session_id(params)?;
    let base = stream::base(ctx.connection_id, session);
    match string_field(params, "subscriptionId")? {
        Some(id) => ctx.subscriptions.cancel(&format!("{base}:{id}")),
        None => ctx.subscriptions.cancel_prefix(&base) > 0,
    };
    Ok(json!({"unsubscribed": true}))
}
