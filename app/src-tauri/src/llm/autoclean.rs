use regex::Regex;
use serde::{Deserialize, Serialize};

use super::polish::PolishEngine;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AutocleanMode {
    Off,
    Fast,
    Polish,
}

impl Default for AutocleanMode {
    fn default() -> Self {
        AutocleanMode::Fast
    }
}

pub struct TierOneRuleSet {
    filler_re: Regex,
    whitespace_re: Regex,
}

impl TierOneRuleSet {
    pub fn new() -> Self {
        Self {
            filler_re: Regex::new(r"\b(um|uh|like|you know)\b[, ]*").unwrap(),
            whitespace_re: Regex::new(r"\s+").unwrap(),
        }
    }

    pub fn apply(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let without_fillers = self.filler_re.replace_all(trimmed, "");
        let cleaned = self.whitespace_re.replace_all(&without_fillers, " ");
        punctuate(&cleaned)
    }
}

pub struct AutocleanService {
    tier_one: TierOneRuleSet,
    mode: std::sync::Mutex<AutocleanMode>,
    polisher: std::sync::Mutex<Option<PolishEngine>>,
}

impl AutocleanService {
    pub fn new() -> Self {
        Self {
            tier_one: TierOneRuleSet::new(),
            mode: std::sync::Mutex::new(AutocleanMode::Fast),
            polisher: std::sync::Mutex::new(PolishEngine::from_env().ok()),
        }
    }

    pub fn set_mode(&self, mode: AutocleanMode) {
        if let Ok(mut guard) = self.mode.lock() {
            *guard = mode;
        }
        if matches!(mode, AutocleanMode::Polish) {
            if let Ok(mut guard) = self.polisher.lock() {
                if guard.is_none() {
                    *guard = PolishEngine::from_env().ok();
                }
            }
        }
    }

    pub fn mode(&self) -> AutocleanMode {
        *self.mode.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn clean(&self, text: &str) -> String {
        let mode = self.mode();
        match mode {
            AutocleanMode::Off => text.to_string(),
            AutocleanMode::Fast => self.tier_one.apply(text),
            AutocleanMode::Polish => {
                let fast = self.tier_one.apply(text);
                let polished = self.polisher.lock().ok().and_then(|mut guard| {
                    guard.as_mut().and_then(|engine| engine.polish(&fast).ok())
                });
                polished.unwrap_or(fast)
            }
        }
    }
}

impl Default for AutocleanService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_mode_trims_and_punctuates() {
        let service = AutocleanService::new();
        service.set_mode(AutocleanMode::Fast);
        let cleaned = service.clean(" um hello  world  ");
        assert_eq!(cleaned, "Hello world.");
    }

    #[test]
    fn polish_without_engine_falls_back() {
        std::env::remove_var("LLAMA_POLISH_CMD");
        let service = AutocleanService::new();
        service.set_mode(AutocleanMode::Polish);
        let cleaned = service.clean(" test phrase");
        assert_eq!(cleaned, "Test phrase.");
    }
}

fn punctuate(value: &str) -> String {
    let mut sentence = value.to_string();
    if !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    let mut chars = sentence.chars();
    if let Some(first) = chars.next() {
        sentence.replace_range(..1, &first.to_uppercase().to_string());
    }
    sentence
}
