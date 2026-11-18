//! A turn's CLI runs at the workspace host, with its own tools and the credentials installed there: the executor's `run_command` starts it and
//! streams its output, and this adapter hands those lines to the driver as a started process.
use async_trait::async_trait;
use rca_cli_driver::application::ports::{Running, Spawner};
use rca_exec_channel::remote::{ExecutorChannel, Streamed};
use rca_exec_channel::RUN_COMMAND;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A turn may think for hours; `run_command`'s default of two minutes would end it.
pub const TURN_TIMEOUT_MS: u64 = 6 * 60 * 60 * 1000;
const STDERR_TAIL: usize = 2000;

/// How the CLI of a turn gets started for a workspace's executor.
pub trait CliLauncher: Send + Sync {
    fn spawner(&self, channel: &Arc<dyn ExecutorChannel>) -> Arc<dyn Spawner>;
}

/// The launcher of the service: the CLI runs where the workspace is.
pub struct AtWorkspace;

impl CliLauncher for AtWorkspace {
    fn spawner(&self, channel: &Arc<dyn ExecutorChannel>) -> Arc<dyn Spawner> {
        Arc::new(HostSpawner { channel: Arc::clone(channel) })
    }
}

/// A launcher that starts every CLI with one spawner, wherever the workspace is: the flows that script the CLI use it.
pub struct Fixed(pub Arc<dyn Spawner>);

impl CliLauncher for Fixed {
    fn spawner(&self, _: &Arc<dyn ExecutorChannel>) -> Arc<dyn Spawner> {
        Arc::clone(&self.0)
    }
}

pub struct HostSpawner {
    channel: Arc<dyn ExecutorChannel>,
}

/// `program` and `args` as one shell word list, each in single quotes, with the input closed (a CLI in `-p` mode must not wait on it).
pub fn shell_line(program: &str, args: &[String]) -> String {
    let quote = |word: &str| format!("'{}'", word.replace('\'', "'\\''"));
    let words: Vec<String> = std::iter::once(program).chain(args.iter().map(String::as_str)).map(quote).collect();
    format!("exec {} < /dev/null", words.join(" "))
}

#[async_trait]
impl Spawner for HostSpawner {
    async fn spawn(&self, program: &str, args: &[String], _cwd: &Path) -> Result<Running, String> {
        if self.channel.is_closed() {
            return Err("the workspace's executor is gone".into());
        }
        let request = json!({"command": shell_line(program, args), "timeout_ms": TURN_TIMEOUT_MS});
        let (stream, mut chunks) = mpsc::unbounded_channel();
        let channel = Arc::clone(&self.channel);
        let call = tokio::spawn(async move { channel.call_stream(RUN_COMMAND, request, stream).await });
        let abort = call.abort_handle();
        let (lines_tx, lines_rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut partial, mut tail) = (String::new(), String::new());
            while let Some(item) = chunks.recv().await {
                let Streamed::Output { stream, data } = item else { continue };
                if stream == "stdout" {
                    partial.push_str(&data);
                    while let Some(end) = partial.find('\n') {
                        let line: String = partial.drain(..=end).collect();
                        let _ = lines_tx.send(line.trim_end_matches(['\n', '\r']).to_string()).await;
                    }
                } else {
                    tail.push_str(&data);
                    if tail.len() > STDERR_TAIL {
                        let mut cut = tail.len() - STDERR_TAIL;
                        while !tail.is_char_boundary(cut) {
                            cut += 1;
                        }
                        tail.drain(..cut);
                    }
                }
            }
            if !partial.is_empty() {
                let _ = lines_tx.send(partial).await;
            }
            drop(lines_tx);
            let outcome = match call.await {
                Ok(Ok(done)) => match (done.result["exit_code"].as_i64(), done.result["timed_out"].as_bool().unwrap_or(false)) {
                    (_, true) => Err("the CLI ran past the turn's time limit".to_string()),
                    (Some(0), _) | (None, _) => Ok(()),
                    (Some(code), _) => Err(if tail.trim().is_empty() { format!("exited with code {code}") } else { tail.trim().to_string() }),
                },
                Ok(Err(e)) => Err(if tail.trim().is_empty() { e.message() } else { format!("{}: {}", e.message(), tail.trim()) }),
                // aborted: the turn was stopped
                Err(_) => Ok(()),
            };
            let _ = done_tx.send(outcome);
        });
        let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || abort.abort());
        Ok(Running {
            lines: Box::pin(futures_util::stream::unfold(lines_rx, |mut rx| async move { rx.recv().await.map(|line| (line, rx)) })),
            finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }),
            kill,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use rca_exec_channel::remote::{ActionOutput, RemoteError};
    use serde_json::{json, Value};
    use std::sync::Mutex;

    /// An executor whose `run_command` streams the scripted chunks, then answers with an exit code.
    struct Executor {
        chunks: Vec<(&'static str, &'static str)>,
        exit: i32,
        seen: Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl ExecutorChannel for Executor {
        async fn call(&self, _: &str, _: Value) -> Result<ActionOutput, RemoteError> {
            unreachable!("a turn streams")
        }
        async fn call_stream(&self, tool: &str, args: Value, stream: mpsc::UnboundedSender<Streamed>) -> Result<ActionOutput, RemoteError> {
            assert_eq!(tool, RUN_COMMAND);
            self.seen.lock().unwrap().push(args);
            for (name, data) in &self.chunks {
                let _ = stream.send(Streamed::Output { stream: name.to_string(), data: data.to_string() });
            }
            Ok(ActionOutput { result: json!({"exit_code": self.exit, "timed_out": false, "cancelled": false, "truncated": false}), output: String::new() })
        }
        fn close(&self, _: u16) {}
        fn is_closed(&self) -> bool {
            false
        }
        async fn closed(&self) {
            std::future::pending::<()>().await
        }
    }

    fn started(chunks: Vec<(&'static str, &'static str)>, exit: i32) -> (Arc<Executor>, Arc<dyn Spawner>) {
        let executor = Arc::new(Executor { chunks, exit, seen: Mutex::default() });
        let channel: Arc<dyn ExecutorChannel> = executor.clone();
        (executor, AtWorkspace.spawner(&channel))
    }

    #[test]
    fn given_arguments_with_quotes_and_spaces_when_made_a_shell_line_then_each_is_one_quoted_word_and_the_input_is_closed() {
        // Given a prompt that holds a quote and a space
        let line = shell_line("claude", &["-p".into(), "it's a test".into()]);
        // Then every word is quoted so the shell sees them as given
        assert_eq!(line, "exec 'claude' '-p' 'it'\\''s a test' < /dev/null");
    }

    #[tokio::test]
    async fn given_a_cli_at_the_host_when_it_prints_lines_split_across_chunks_then_whole_lines_arrive_and_the_run_ends_well() {
        // Given an executor streaming two lines in three chunks and a stderr note
        let (executor, spawner) = started(vec![("stdout", "{\"a\":1}\n{\"b\""), ("stderr", "warn\n"), ("stdout", ":2}\n")], 0);
        // When the CLI is started
        let process = spawner.spawn("claude", &["-p".into(), "go".into()], Path::new(".")).await.unwrap();
        let lines: Vec<String> = process.lines.collect().await;
        // Then whole lines came out, the run ended without error, and the command ran with the turn's time limit
        assert_eq!(lines, vec!["{\"a\":1}", "{\"b\":2}"]);
        assert_eq!(process.finish.await, Ok(()));
        assert_eq!(executor.seen.lock().unwrap()[0]["timeout_ms"], TURN_TIMEOUT_MS);
    }

    #[tokio::test]
    async fn given_a_cli_that_exits_non_zero_when_it_ends_then_the_run_fails_with_its_stderr() {
        // Given a CLI that says why on stderr and exits 127
        let (_, spawner) = started(vec![("stderr", "claude: command not found\n")], 127);
        let process = spawner.spawn("claude", &[], Path::new(".")).await.unwrap();
        // When it ends
        let finished = process.finish.await;
        // Then the failure names the reason
        assert_eq!(finished, Err("claude: command not found".to_string()));
    }
}
