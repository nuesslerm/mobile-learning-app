//! Every scalar environment setting the service reads, parsed with its default. The one place an env name and its default live;
//! `from_lookup` takes the reader (the `Environment` port's `var`) so a test feeds a map and never touches the process environment.
use rca_cli_driver::domain::PROVIDERS_ROOT;
use std::path::PathBuf;

pub const MODEL_API_BASE_URL: &str = "https://api.kimi.com/coding/v1";
pub const MODEL_API_MODEL: &str = "kimi-for-coding";

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub port: u16,
    pub opencode_server_url: String,
    pub opencode_server_username: String,
    pub opencode_server_password: Option<String>,
    pub opencode_text_model: String,
    pub opencode_vision_model: String,
    pub opencode_tamil_marker_model: String,
    pub opencode_default_model: Option<String>,
    pub opencode_response_timeout_ms: u64,
    pub session_ttl_hours: u64,
    pub ask_root: PathBuf,
    pub dispatch_run_log: Option<PathBuf>,
    pub worktree_base_dir: String,
    pub edit_stats_ords_base_url: String,
    pub health_reports_ords_user: String,
    pub health_reports_ords_password: String,
    /// Read as set (an empty value counts): each caller applies its own fallback, as it did when it read the variable itself.
    pub fork_dir: Option<String>,
    pub opencode_cwd: Option<String>,
    pub opencode_db_path: Option<String>,
    pub telegram_bot_token: String,
    pub agent_public_url: Option<String>,
    pub tamil_check_scratch_dir: Option<PathBuf>,
    pub voice_edit_agent: Option<String>,
    pub voice_edit_prompt_v1: bool,
    pub voice_edit_turn_timeout_ms: f64,
    pub voice_edit_session_max_turns: f64,
    pub opencode_complex_model: String,
    pub pool_cooldowns_ords_base_url: String,
    pub pdftotext_path: String,
    pub tesseract_path: String,
    pub allowed_origins: Option<String>,
    pub api_secret: Option<String>,
    pub git_commit: String,
    pub default_cwd: Option<String>,
    pub hard_timeout_ms: u64,
    pub state_dir: Option<String>,
    pub exec_root: String,
    pub stt: SttSettings,
    pub agent_cli: Option<String>,
    pub ladder_ignore_stats: bool,
    pub claude_session_id: Option<String>,
    /// `RCA_PROVIDERS_ROOT`: where the provider CLIs keep their logins.
    pub providers_root: String,
    pub model_api_base_url: String,
    pub model_api_model: String,
    pub kimi_api_key: Option<String>,
}

/// The speech-to-text engine's variables, each as set; the engine applies its own fallbacks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SttSettings {
    pub url: Option<String>,
    pub engine: Option<String>,
    pub groq_api_key: Option<String>,
    pub groq_model: Option<String>,
    pub whisper_threads: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub whisper_cpp_binary: Option<String>,
    pub whisper_model_name: Option<String>,
    pub whisper_model_path: Option<String>,
}

impl Settings {
    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Self {
        let set = |name: &str| get(name).filter(|v| !v.is_empty());
        let text = |name: &str, default: &str| set(name).unwrap_or_else(|| default.into());
        let number = |name: &str, default: u64| get(name).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        Settings {
            port: get("PORT").and_then(|p| p.parse().ok()).unwrap_or(8787),
            opencode_server_url: text("OPENCODE_SERVER_URL", "http://localhost:4096"),
            opencode_server_username: text("OPENCODE_SERVER_USERNAME", "opencode"),
            opencode_server_password: set("OPENCODE_SERVER_PASSWORD"),
            opencode_text_model: text("OPENCODE_TEXT_MODEL", "kimi-code-plan-global/kimi-for-coding-highspeed"),
            opencode_vision_model: text("OPENCODE_VISION_MODEL", "minimax/MiniMax-M3"),
            opencode_tamil_marker_model: text("OPENCODE_TAMIL_MARKER_MODEL", "opencode-go/gpt-6-luna"),
            opencode_default_model: set("OPENCODE_DEFAULT_MODEL"),
            opencode_response_timeout_ms: get("OPENCODE_RESPONSE_TIMEOUT_MS").and_then(|v| v.parse().ok()).unwrap_or(60_000),
            session_ttl_hours: number("RCA_SESSION_TTL_HOURS", 24),
            ask_root: set("RCA_ASK_ROOT").map_or_else(|| PathBuf::from("/workspace/rca-asks"), PathBuf::from),
            dispatch_run_log: set("OPENCODE_DISPATCH_RUN_LOG").map(PathBuf::from),
            worktree_base_dir: get("WORKTREE_BASE_DIR").unwrap_or_else(|| "/tmp/worktrees".into()),
            edit_stats_ords_base_url: get("EDIT_STATS_ORDS_BASE_URL").unwrap_or_default(),
            health_reports_ords_user: text("HEALTH_REPORTS_ORDS_USER", "health_rpt"),
            health_reports_ords_password: get("HEALTH_REPORTS_ORDS_PASSWORD").unwrap_or_default(),
            fork_dir: get("FORK_DIR"),
            opencode_cwd: get("OPENCODE_CWD"),
            opencode_db_path: get("OPENCODE_DB_PATH"),
            telegram_bot_token: get("TELEGRAM_BOT_TOKEN").unwrap_or_default(),
            agent_public_url: get("AGENT_PUBLIC_URL"),
            tamil_check_scratch_dir: set("TAMIL_CHECK_SCRATCH_DIR").map(PathBuf::from),
            voice_edit_agent: get("VOICE_EDIT_AGENT"),
            voice_edit_prompt_v1: get("VOICE_EDIT_PROMPT_VARIANT").as_deref() == Some("v1"),
            voice_edit_turn_timeout_ms: get("VOICE_EDIT_TURN_TIMEOUT_MS").and_then(|v| v.trim().parse().ok()).unwrap_or(45_000.0),
            voice_edit_session_max_turns: get("VOICE_EDIT_SESSION_MAX_TURNS").and_then(|v| v.trim().parse().ok()).unwrap_or(20.0),
            opencode_complex_model: text("OPENCODE_COMPLEX_MODEL", "kimi-code-plan-global/kimi-for-coding"),
            pool_cooldowns_ords_base_url: get("POOL_COOLDOWNS_ORDS_BASE_URL").unwrap_or_default().trim().to_string(),
            pdftotext_path: text("PDFTOTEXT_PATH", "pdftotext"),
            tesseract_path: text("TESSERACT_PATH", "tesseract"),
            allowed_origins: set("CHAT_AGENT_ALLOWED_ORIGINS"),
            api_secret: set("CHAT_AGENT_API_SECRET"),
            git_commit: text("GIT_COMMIT", "unknown"),
            default_cwd: set("RCA_DEFAULT_CWD"),
            hard_timeout_ms: set("OPENCODE_HARD_TIMEOUT_MS").and_then(|v| v.parse().ok()).unwrap_or(600_000),
            state_dir: set("RCA_STATE_DIR"),
            exec_root: text("RCA_EXEC_ROOT", "/workspace"),
            agent_cli: get("AGENT_CLI"),
            ladder_ignore_stats: get("OPENCODE_LADDER_IGNORE_STATS").as_deref() == Some("1"),
            claude_session_id: ["CLAUDE_CODE_SESSION_ID", "CLAUDE_SESSION_ID"].iter().find_map(|k| set(k)),
            providers_root: text("RCA_PROVIDERS_ROOT", PROVIDERS_ROOT),
            model_api_base_url: text("RCA_MODEL_API_BASE_URL", MODEL_API_BASE_URL),
            model_api_model: text("RCA_MODEL_API_MODEL", MODEL_API_MODEL),
            kimi_api_key: set("KIMI_API_KEY"),
            stt: SttSettings {
                url: get("STT_URL"),
                engine: get("STT_ENGINE"),
                groq_api_key: get("GROQ_API_KEY"),
                groq_model: get("GROQ_STT_MODEL"),
                whisper_threads: get("WHISPER_THREADS"),
                ffmpeg_path: get("FFMPEG_PATH"),
                whisper_cpp_binary: get("WHISPER_CPP_BINARY"),
                whisper_model_name: get("WHISPER_MODEL_NAME"),
                whisper_model_path: get("WHISPER_MODEL_PATH"),
            },
        }
    }

    /// Every variable `from_lookup` asks for, from the code's own reads.
    pub fn names() -> Vec<String> {
        let asked = std::cell::RefCell::new(Vec::new());
        Settings::from_lookup(&|name| {
            asked.borrow_mut().push(name.to_string());
            None
        });
        let mut names = asked.into_inner();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn from(pairs: &[(&str, &str)]) -> Settings {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Settings::from_lookup(&|name| map.get(name).cloned())
    }

    #[test]
    fn given_an_empty_environment_when_read_then_every_default_the_service_had_applies() {
        let s = from(&[]);
        assert_eq!((s.port, s.opencode_server_url.as_str(), s.opencode_server_username.as_str()), (8787, "http://localhost:4096", "opencode"));
        assert_eq!((s.opencode_server_password, s.opencode_default_model, s.dispatch_run_log), (None, None, None));
        assert_eq!((s.opencode_response_timeout_ms, s.session_ttl_hours, s.ask_root), (60_000, 24, PathBuf::from("/workspace/rca-asks")));
        assert_eq!((s.worktree_base_dir.as_str(), s.health_reports_ords_user.as_str()), ("/tmp/worktrees", "health_rpt"));
        assert_eq!((s.opencode_vision_model.as_str(), s.opencode_tamil_marker_model.as_str()), ("minimax/MiniMax-M3", "opencode-go/gpt-6-luna"));
    }

    #[test]
    fn given_set_values_when_read_then_they_win_and_an_empty_or_unparsable_one_falls_back_to_the_default() {
        let s = from(&[("PORT", "9000"), ("OPENCODE_SERVER_URL", ""), ("OPENCODE_SERVER_PASSWORD", "pw"), ("OPENCODE_RESPONSE_TIMEOUT_MS", "soon"), ("RCA_SESSION_TTL_HOURS", "2"), ("RCA_ASK_ROOT", "/data/asks"), ("OPENCODE_DISPATCH_RUN_LOG", "/data/runs.jsonl")]);
        assert_eq!((s.port, s.opencode_server_url.as_str(), s.opencode_server_password.as_deref()), (9000, "http://localhost:4096", Some("pw")));
        assert_eq!((s.opencode_response_timeout_ms, s.session_ttl_hours), (60_000, 2));
        assert_eq!((s.ask_root, s.dispatch_run_log), (PathBuf::from("/data/asks"), Some(PathBuf::from("/data/runs.jsonl"))));
    }

    #[test]
    fn given_the_switches_when_read_then_an_empty_voice_agent_is_kept_as_set() {
        assert_eq!((from(&[]).voice_edit_agent, from(&[("VOICE_EDIT_AGENT", "")]).voice_edit_agent), (None, Some(String::new())));
        assert!(from(&[("VOICE_EDIT_PROMPT_VARIANT", "v1")]).voice_edit_prompt_v1 && !from(&[]).voice_edit_prompt_v1);
        assert_eq!(from(&[("TAMIL_CHECK_SCRATCH_DIR", "")]).tamil_check_scratch_dir, None);
    }

    #[test]
    fn given_each_name_the_settings_read_when_only_it_is_set_then_the_settings_change_and_unset_they_are_the_defaults() {
        let defaults = from(&[]);
        let names = Settings::names();
        assert!(names.len() > 50, "{names:?}");
        for name in &names {
            let value = match name.as_str() {
                n if n.ends_with("_VARIANT") => "v1".to_string(),
                n if n.ends_with("_STATS") => "1".to_string(),
                n if n == "PORT" || n.ends_with("_MS") || n.ends_with("_HOURS") || n.ends_with("_TURNS") => "7".to_string(),
                n => format!("sentinel-{n}"),
            };
            let only = |asked: &str| (asked == name).then(|| value.clone());
            let read = Settings::from_lookup(&|asked| only(asked));
            assert_ne!(read, defaults, "{name} does not reach the settings");
        }
    }
}
