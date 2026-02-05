use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

#[cfg(feature = "llama-polish")]
use std::sync::Arc;

#[cfg(feature = "llama-polish")]
use llama_cpp::{standard_sampler::StandardSampler, LlamaModel, LlamaParams, SessionParams};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const ENV_CMD: &str = "LLAMA_POLISH_CMD";
const ENV_ARGS: &str = "LLAMA_POLISH_ARGS";
const ENV_TIMEOUT: &str = "LLAMA_POLISH_TIMEOUT_SECS";
const ENV_MODEL: &str = "LLAMA_POLISH_MODEL";
const MAX_GENERATED_TOKENS: usize = 256;
const MAX_GENERATED_CHARS: usize = 1024;
const END_SENTINEL: &str = "<END>";

pub struct PolishEngine {
    backend: PolishBackend,
}

enum PolishBackend {
    Command(CommandBackend),
    #[cfg(feature = "llama-polish")]
    Llama(LlamaBackend),
}

struct CommandBackend {
    path: PathBuf,
    args: Vec<String>,
    timeout: Duration,
}

#[cfg(feature = "llama-polish")]
struct LlamaBackend {
    model: Arc<LlamaModel>,
    instructions: String,
    max_tokens: usize,
    max_chars: usize,
}

impl PolishEngine {
    pub fn from_env() -> Result<Self> {
        if let Ok(cmd) = std::env::var(ENV_CMD) {
            return Ok(Self {
                backend: PolishBackend::Command(CommandBackend::from_env(PathBuf::from(cmd))?),
            });
        }

        #[cfg(feature = "llama-polish")]
        {
            if let Ok(model_path) = std::env::var(ENV_MODEL) {
                let backend = LlamaBackend::new(PathBuf::from(model_path))?;
                return Ok(Self {
                    backend: PolishBackend::Llama(backend),
                });
            }
        }

        Err(anyhow!(
            "{} not set and no {} available; cannot initialize polish backend",
            ENV_CMD,
            ENV_MODEL
        ))
    }

    pub fn polish(&self, input: &str) -> Result<String> {
        match &self.backend {
            PolishBackend::Command(cmd) => cmd.run(input),
            #[cfg(feature = "llama-polish")]
            PolishBackend::Llama(llm) => llm.polish(input),
        }
    }
}

impl CommandBackend {
    fn from_env(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("{} points to missing binary: {:?}", ENV_CMD, path));
        }

        let args = std::env::var(ENV_ARGS)
            .map(|value| value.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        let timeout = std::env::var(ENV_TIMEOUT)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT);

        Ok(Self {
            path,
            args,
            timeout,
        })
    }

    fn run(&self, input: &str) -> Result<String> {
        let mut child = Command::new(&self.path)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn polish command {:?}", self.path))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(input.as_bytes())
                .context("failed to write prompt to polish command stdin")?;
        }

        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().context("failed polling polish command")? {
                if !status.success() {
                    let stderr = child
                        .stderr
                        .take()
                        .and_then(|mut pipe| {
                            let mut buf = Vec::new();
                            std::io::Read::read_to_end(&mut pipe, &mut buf).ok()?;
                            Some(buf)
                        })
                        .unwrap_or_default();
                    let message = String::from_utf8_lossy(&stderr);
                    return Err(anyhow!(
                        "polish command exited with status {:?}: {}",
                        status.code(),
                        message
                    ));
                }
                let mut stdout = child
                    .stdout
                    .take()
                    .context("stdout handle unavailable after process exit")?;
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut stdout, &mut buf)
                    .context("failed reading polish command stdout")?;
                let text =
                    String::from_utf8(buf).context("polish command returned non-UTF8 text")?;
                return Ok(text.trim().to_string());
            }

            if start.elapsed() > self.timeout {
                let _ = child.kill();
                return Err(anyhow!(
                    "polish command exceeded {:?} timeout",
                    self.timeout
                ));
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(feature = "llama-polish")]
impl LlamaBackend {
    fn new(model_path: PathBuf) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!(
                "{} points to missing model file: {:?}",
                ENV_MODEL,
                model_path
            ));
        }

        let params = LlamaParams::default();
        let model = LlamaModel::load_from_file(&model_path, params)
            .map_err(|err| anyhow!("failed to load llama polish model: {err:?}"))?;

        let instructions = "You are a meticulous speech cleanup assistant. Remove disfluencies, fix punctuation, and preserve the speaker's intent. Reply with the polished transcript only and end your response with <END>.".to_string();

        Ok(Self {
            model: Arc::new(model),
            instructions,
            max_tokens: MAX_GENERATED_TOKENS,
            max_chars: MAX_GENERATED_CHARS,
        })
    }

    fn polish(&self, input: &str) -> Result<String> {
        let mut session = self
            .model
            .create_session(SessionParams::default())
            .map_err(|err| anyhow!("failed to create llama session: {err:?}"))?;

        let prompt = format!(
            "{instructions}

Transcript:
{input}

Polished transcript (end with {END_SENTINEL}):
",
            instructions = self.instructions,
            input = input,
        );

        session
            .advance_context(&prompt)
            .map_err(|err| anyhow!("failed to advance llama context: {err}"))?;

        let sampler = StandardSampler::default();
        let mut stream = session
            .start_completing_with(sampler, self.max_tokens)
            .map_err(|err| anyhow!("failed to start llama completion: {err:?}"))?
            .into_strings();

        let mut output = String::new();
        while let Some(chunk) = stream.next() {
            if chunk.is_empty() {
                continue;
            }
            output.push_str(&chunk);
            if output.contains(END_SENTINEL) || output.len() >= self.max_chars {
                break;
            }
        }
        drop(stream);

        let polished = output
            .split(END_SENTINEL)
            .next()
            .unwrap_or(&output)
            .trim()
            .to_string();

        if polished.is_empty() {
            Err(anyhow!("llama polish produced an empty result"))
        } else {
            Ok(polished)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn command_backend_accepts_defaults() {
        std::env::remove_var(ENV_ARGS);
        std::env::remove_var(ENV_TIMEOUT);
        let backend = CommandBackend::from_env(PathBuf::from("/bin/sh"));
        assert!(backend.is_ok());
    }

    #[test]
    fn command_backend_missing_binary_errors() {
        let backend = CommandBackend::from_env(PathBuf::from("/definitely/missing/binary"));
        assert!(backend.is_err());
    }
}
