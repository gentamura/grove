use crate::models::{
    ActivityKind, CodingAgent, CodingSession, CodingSubagent, ConversationMessage,
    ConversationRole, SessionActivity, SessionScan, SessionStatus,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    ffi::OsStr,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use thiserror::Error;
use walkdir::WalkDir;

const ACTIVE_WINDOW: Duration = Duration::from_secs(90);
const WAITING_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_ACTIVITIES: usize = 6;
const PREVIEW_LIMIT: usize = 180;

#[derive(Debug, Clone)]
pub struct SessionRoots {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("failed to inspect {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not find the transcript for session {session_id}")]
    SessionNotFound { session_id: String },
}

#[derive(Default)]
struct SessionAccumulator {
    id: Option<String>,
    agent_id: Option<String>,
    entrypoint: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    slug: Option<String>,
    ai_title: Option<String>,
    last_prompt: Option<String>,
    first_prompt: Option<String>,
    last_tool: Option<String>,
    last_turn_finished: bool,
    first_timestamp: Option<DateTime<Utc>>,
    last_timestamp: Option<DateTime<Utc>>,
    message_count: usize,
    activities: VecDeque<SessionActivity>,
    spawned_tool_ids: Vec<String>,
}

struct ParsedSubagent {
    model: CodingSubagent,
    tool_use_id: Option<String>,
    spawned_tool_ids: Vec<String>,
}

#[derive(Default)]
struct SubagentScan {
    subagents: Vec<CodingSubagent>,
    skipped_files: usize,
    warnings: Vec<String>,
}

#[derive(Default)]
struct CodexAccumulator {
    id: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    title: Option<String>,
    first_prompt: Option<String>,
    last_prompt: Option<String>,
    last_tool: Option<String>,
    last_turn_finished: bool,
    is_subagent: bool,
    first_timestamp: Option<DateTime<Utc>>,
    last_timestamp: Option<DateTime<Utc>>,
    message_count: usize,
    activities: VecDeque<SessionActivity>,
}

impl SessionAccumulator {
    fn consume(&mut self, value: Value) {
        self.id = string_field(&value, "sessionId").or_else(|| self.id.take());
        self.agent_id = string_field(&value, "agentId").or_else(|| self.agent_id.take());
        self.entrypoint = string_field(&value, "entrypoint").or_else(|| self.entrypoint.take());
        self.cwd = string_field(&value, "cwd").or_else(|| self.cwd.take());
        self.git_branch = string_field(&value, "gitBranch").or_else(|| self.git_branch.take());
        self.slug = string_field(&value, "slug").or_else(|| self.slug.take());

        let timestamp = string_field(&value, "timestamp")
            .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
            .map(|parsed| parsed.with_timezone(&Utc));
        if let Some(timestamp) = timestamp {
            self.first_timestamp = Some(
                self.first_timestamp
                    .map_or(timestamp, |current| current.min(timestamp)),
            );
            self.last_timestamp = Some(
                self.last_timestamp
                    .map_or(timestamp, |current| current.max(timestamp)),
            );
        }

        match value.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                self.ai_title = string_field(&value, "aiTitle").or_else(|| self.ai_title.take());
            }
            Some("last-prompt") => {
                if let Some(prompt) = string_field(&value, "lastPrompt")
                    .and_then(|raw| displayable_user_text(&raw, PREVIEW_LIMIT))
                {
                    self.last_prompt = Some(prompt);
                }
            }
            Some("user") => self.consume_user(&value, timestamp),
            Some("assistant") => self.consume_assistant(&value, timestamp),
            _ => {}
        }
    }

    fn consume_user(&mut self, value: &Value, timestamp: Option<DateTime<Utc>>) {
        let Some(message) = value.get("message") else {
            return;
        };
        let Some(prompt) = extract_user_text(message) else {
            return;
        };
        self.message_count += 1;
        self.last_turn_finished = false;
        self.first_prompt.get_or_insert_with(|| prompt.clone());
        self.last_prompt = Some(prompt.clone());
        self.push_activity(SessionActivity {
            kind: ActivityKind::Prompt,
            label: "Prompt".into(),
            detail: Some(prompt),
            timestamp: timestamp.map(format_timestamp),
        });
    }

    fn consume_assistant(&mut self, value: &Value, timestamp: Option<DateTime<Utc>>) {
        self.message_count += 1;
        let Some(message) = value.get("message") else {
            return;
        };

        if let Some(stop_reason) = string_field(message, "stop_reason") {
            self.last_turn_finished = stop_reason == "end_turn";
        }

        let Some(content) = message.get("content").and_then(Value::as_array) else {
            return;
        };

        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let name = string_field(item, "name").unwrap_or_else(|| "Tool".into());
                    if matches!(name.as_str(), "Agent" | "Task")
                        && let Some(tool_use_id) = string_field(item, "id")
                    {
                        self.spawned_tool_ids.push(tool_use_id);
                    }
                    self.last_tool = Some(name.clone());
                    self.push_activity(SessionActivity {
                        kind: ActivityKind::Tool,
                        label: name.clone(),
                        detail: tool_detail(&name, item.get("input")),
                        timestamp: timestamp.map(format_timestamp),
                    });
                }
                Some("text") => {
                    let text = string_field(item, "text")
                        .map(|raw| compact_text(&raw, PREVIEW_LIMIT))
                        .filter(|raw| !raw.is_empty());
                    if let Some(text) = text {
                        self.push_activity(SessionActivity {
                            kind: ActivityKind::Response,
                            label: "Claude".into(),
                            detail: Some(text),
                            timestamp: timestamp.map(format_timestamp),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn push_activity(&mut self, activity: SessionActivity) {
        if self.activities.len() == MAX_ACTIVITIES {
            self.activities.pop_front();
        }
        self.activities.push_back(activity);
    }
}

pub fn scan_claude_sessions_at(root: &Path, now: SystemTime) -> Result<SessionScan, ScanError> {
    let mut sessions = Vec::new();
    let mut skipped_files = 0;
    let mut warnings = Vec::new();

    if root.exists() {
        for entry in WalkDir::new(root)
            .min_depth(2)
            .max_depth(3)
            .follow_links(false)
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    skipped_files += 1;
                    warnings.push(format!("Could not inspect a session path: {error}"));
                    continue;
                }
            };
            if !entry.file_type().is_file()
                || entry.path().extension() != Some(OsStr::new("jsonl"))
                || entry.file_name().to_string_lossy().starts_with("agent-")
            {
                continue;
            }

            match parse_session_file(entry.path(), now) {
                Ok(Some(mut session)) => {
                    let subagent_root = entry.path().with_extension("").join("subagents");
                    let subagent_scan = scan_subagents(&subagent_root, now);
                    session.subagents = subagent_scan.subagents;
                    skipped_files += subagent_scan.skipped_files;
                    warnings.extend(subagent_scan.warnings);
                    sessions.push(session);
                }
                Ok(None) => skipped_files += 1,
                Err(error) => {
                    skipped_files += 1;
                    warnings.push(error.to_string());
                }
            }
        }
    }

    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    Ok(SessionScan {
        sessions,
        scanned_at: format_timestamp(DateTime::<Utc>::from(now)),
        source_roots: vec![root.to_string_lossy().into_owned()],
        skipped_files,
        warnings,
    })
}

pub fn scan_sessions_at(roots: &SessionRoots, now: SystemTime) -> Result<SessionScan, ScanError> {
    let mut claude = scan_claude_sessions_at(&roots.claude, now)?;
    let codex = scan_codex_sessions_at(&roots.codex, now)?;
    claude.sessions.extend(codex.sessions);
    claude
        .sessions
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    claude.source_roots.extend(codex.source_roots);
    claude.skipped_files += codex.skipped_files;
    claude.warnings.extend(codex.warnings);
    Ok(claude)
}

pub fn scan_codex_sessions_at(root: &Path, now: SystemTime) -> Result<SessionScan, ScanError> {
    let mut sessions = Vec::new();
    let mut skipped_files = 0;
    let mut warnings = Vec::new();

    for (directory, archived) in [
        (root.join("sessions"), false),
        (root.join("archived_sessions"), true),
    ] {
        if !directory.exists() {
            continue;
        }
        for entry in WalkDir::new(&directory)
            .min_depth(1)
            .max_depth(6)
            .follow_links(false)
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    skipped_files += 1;
                    warnings.push(format!("Could not inspect a Codex session path: {error}"));
                    continue;
                }
            };
            if !entry.file_type().is_file() || entry.path().extension() != Some(OsStr::new("jsonl"))
            {
                continue;
            }
            match parse_codex_session_file(entry.path(), now, archived) {
                Ok(Some(session)) => sessions.push(session),
                Ok(None) => {}
                Err(error) => {
                    skipped_files += 1;
                    warnings.push(error.to_string());
                }
            }
        }
    }

    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(SessionScan {
        sessions,
        scanned_at: format_timestamp(DateTime::<Utc>::from(now)),
        source_roots: vec![root.to_string_lossy().into_owned()],
        skipped_files,
        warnings,
    })
}

pub fn load_session_messages_for(
    roots: &SessionRoots,
    provider: CodingAgent,
    session_id: &str,
) -> Result<Vec<ConversationMessage>, ScanError> {
    match provider {
        CodingAgent::ClaudeCode => load_session_messages_at(&roots.claude, session_id),
        CodingAgent::Codex => load_codex_session_messages_at(&roots.codex, session_id),
    }
}

pub fn load_session_messages_at(
    root: &Path,
    session_id: &str,
) -> Result<Vec<ConversationMessage>, ScanError> {
    let transcript = WalkDir::new(root)
        .min_depth(2)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_type().is_file()
                && entry.path().extension() == Some(OsStr::new("jsonl"))
                && entry.path().file_stem() == Some(OsStr::new(session_id))
        })
        .map(|entry| entry.into_path())
        .ok_or_else(|| ScanError::SessionNotFound {
            session_id: session_id.to_owned(),
        })?;
    load_messages_from_file(&transcript)
}

fn load_messages_from_file(path: &Path) -> Result<Vec<ConversationMessage>, ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut messages = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|source| ScanError::Read {
            path: path.to_owned(),
            source,
        })?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let timestamp = string_field(&value, "timestamp");
        match value.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(text) = value.get("message").and_then(extract_full_user_text) {
                    messages.push(ConversationMessage {
                        role: ConversationRole::User,
                        text,
                        timestamp,
                    });
                }
            }
            Some("assistant") => {
                if let Some(text) = value.get("message").and_then(extract_full_assistant_text) {
                    messages.push(ConversationMessage {
                        role: ConversationRole::Assistant,
                        text,
                        timestamp,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

fn parse_session_file(path: &Path, now: SystemTime) -> Result<Option<CodingSession>, ScanError> {
    let (accumulator, fallback_updated) = read_session_file(path, now)?;

    let Some(file_stem) = path.file_stem() else {
        return Ok(None);
    };
    let fallback_id = file_stem.to_string_lossy().into_owned();
    let id = accumulator.id.clone().unwrap_or(fallback_id);
    if id.starts_with("agent-") {
        return Ok(None);
    }

    let updated = accumulator
        .last_timestamp
        .unwrap_or_else(|| DateTime::<Utc>::from(fallback_updated));
    let updated_system: SystemTime = updated.into();
    let elapsed = now.duration_since(updated_system).unwrap_or(Duration::ZERO);
    let status = classify_status(elapsed, accumulator.last_turn_finished);

    let cwd = accumulator
        .cwd
        .clone()
        .unwrap_or_else(|| inferred_project_path(path));
    let project_name = Path::new(&cwd)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unknown project".into());
    let title = accumulator
        .ai_title
        .clone()
        .or_else(|| accumulator.first_prompt.clone())
        .or_else(|| accumulator.last_prompt.clone())
        .or_else(|| accumulator.slug.clone())
        .map(|title| compact_text(&title, 96))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled session".into());

    Ok(Some(CodingSession {
        id,
        provider: CodingAgent::ClaudeCode,
        title,
        project_name,
        cwd,
        git_branch: accumulator.git_branch,
        slug: accumulator.slug,
        status,
        updated_at: format_timestamp(updated),
        started_at: accumulator.first_timestamp.map(format_timestamp),
        message_count: accumulator.message_count,
        last_prompt: accumulator.last_prompt,
        last_tool: accumulator.last_tool,
        activities: accumulator.activities.into_iter().rev().collect(),
        subagents: vec![],
    }))
}

impl CodexAccumulator {
    fn consume(&mut self, value: Value) {
        let timestamp = string_field(&value, "timestamp")
            .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
            .map(|parsed| parsed.with_timezone(&Utc));
        if let Some(timestamp) = timestamp {
            self.first_timestamp = Some(
                self.first_timestamp
                    .map_or(timestamp, |current| current.min(timestamp)),
            );
            self.last_timestamp = Some(
                self.last_timestamp
                    .map_or(timestamp, |current| current.max(timestamp)),
            );
        }

        let Some(payload) = value.get("payload") else {
            return;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                self.id = string_field(payload, "id")
                    .or_else(|| string_field(payload, "session_id"))
                    .or_else(|| self.id.take());
                self.cwd = string_field(payload, "cwd").or_else(|| self.cwd.take());
                self.git_branch = payload
                    .get("git")
                    .and_then(|git| string_field(git, "branch"))
                    .or_else(|| self.git_branch.take());
                self.is_subagent = payload.get("source").is_some_and(Value::is_object);
            }
            Some("event_msg") => self.consume_event(payload),
            Some("response_item") => self.consume_response_item(payload, timestamp),
            _ => {}
        }
    }

    fn consume_event(&mut self, payload: &Value) {
        match payload.get("type").and_then(Value::as_str) {
            Some("task_started") => self.last_turn_finished = false,
            Some("task_complete" | "turn_aborted") => self.last_turn_finished = true,
            Some("thread_name_updated") => {
                self.title = string_field(payload, "thread_name")
                    .or_else(|| string_field(payload, "name"))
                    .or_else(|| self.title.take());
            }
            _ => {}
        }
    }

    fn consume_response_item(&mut self, payload: &Value, timestamp: Option<DateTime<Utc>>) {
        match payload.get("type").and_then(Value::as_str) {
            Some("message") => match payload.get("role").and_then(Value::as_str) {
                Some("user") => {
                    if let Some(prompt) = codex_message_text(payload, "input_text")
                        .and_then(|text| displayable_codex_user_text(&text))
                    {
                        self.message_count += 1;
                        self.first_prompt.get_or_insert_with(|| prompt.clone());
                        self.last_prompt = Some(prompt.clone());
                        self.push_activity(SessionActivity {
                            kind: ActivityKind::Prompt,
                            label: "Prompt".into(),
                            detail: Some(compact_text(&prompt, PREVIEW_LIMIT)),
                            timestamp: timestamp.map(format_timestamp),
                        });
                    }
                }
                Some("assistant") => {
                    if let Some(response) = codex_message_text(payload, "output_text") {
                        self.message_count += 1;
                        self.push_activity(SessionActivity {
                            kind: ActivityKind::Response,
                            label: "Codex".into(),
                            detail: Some(compact_text(&response, PREVIEW_LIMIT)),
                            timestamp: timestamp.map(format_timestamp),
                        });
                    }
                }
                _ => {}
            },
            Some("function_call" | "custom_tool_call" | "tool_search_call") => {
                let name = string_field(payload, "name")
                    .or_else(|| string_field(payload, "namespace"))
                    .unwrap_or_else(|| "Tool".into());
                self.last_tool = Some(name.clone());
                self.push_activity(SessionActivity {
                    kind: ActivityKind::Tool,
                    label: name,
                    detail: None,
                    timestamp: timestamp.map(format_timestamp),
                });
            }
            _ => {}
        }
    }

    fn push_activity(&mut self, activity: SessionActivity) {
        if self.activities.len() == MAX_ACTIVITIES {
            self.activities.pop_front();
        }
        self.activities.push_back(activity);
    }
}

fn parse_codex_session_file(
    path: &Path,
    now: SystemTime,
    archived: bool,
) -> Result<Option<CodingSession>, ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Read {
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| ScanError::Metadata {
        path: path.to_owned(),
        source,
    })?;
    let fallback_updated = metadata.modified().unwrap_or(now);
    let mut accumulator = CodexAccumulator::default();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|source| ScanError::Read {
            path: path.to_owned(),
            source,
        })?;
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            accumulator.consume(value);
        }
    }

    if accumulator.is_subagent {
        return Ok(None);
    }
    let Some(id) = accumulator.id else {
        return Ok(None);
    };
    let updated = accumulator
        .last_timestamp
        .unwrap_or_else(|| DateTime::<Utc>::from(fallback_updated));
    let elapsed = now
        .duration_since(SystemTime::from(updated))
        .unwrap_or(Duration::ZERO);
    let cwd = accumulator.cwd.unwrap_or_else(|| "Unknown project".into());
    let project_name = Path::new(&cwd)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unknown project".into());
    let title = accumulator
        .title
        .or(accumulator.first_prompt)
        .or_else(|| accumulator.last_prompt.clone())
        .map(|title| compact_text(&title, 96))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled Codex session".into());
    let status = if archived {
        SessionStatus::Idle
    } else {
        classify_status(elapsed, accumulator.last_turn_finished)
    };

    Ok(Some(CodingSession {
        id,
        provider: CodingAgent::Codex,
        title,
        project_name,
        cwd,
        git_branch: accumulator.git_branch,
        slug: None,
        status,
        updated_at: format_timestamp(updated),
        started_at: accumulator.first_timestamp.map(format_timestamp),
        message_count: accumulator.message_count,
        last_prompt: accumulator.last_prompt,
        last_tool: accumulator.last_tool,
        activities: accumulator.activities.into_iter().rev().collect(),
        subagents: vec![],
    }))
}

fn load_codex_session_messages_at(
    root: &Path,
    session_id: &str,
) -> Result<Vec<ConversationMessage>, ScanError> {
    for directory in [root.join("sessions"), root.join("archived_sessions")] {
        if !directory.exists() {
            continue;
        }
        if let Some(path) = WalkDir::new(directory)
            .min_depth(1)
            .max_depth(6)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.file_type().is_file()
                    && entry.path().extension() == Some(OsStr::new("jsonl"))
                    && entry
                        .path()
                        .file_stem()
                        .is_some_and(|stem| stem.to_string_lossy().ends_with(session_id))
            })
            .map(|entry| entry.into_path())
        {
            return load_codex_messages_from_file(&path);
        }
    }
    Err(ScanError::SessionNotFound {
        session_id: session_id.to_owned(),
    })
}

fn load_codex_messages_from_file(path: &Path) -> Result<Vec<ConversationMessage>, ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut messages = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|source| ScanError::Read {
            path: path.to_owned(),
            source,
        })?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }
        let Some(payload) = value.get("payload") else {
            continue;
        };
        let (role, content_type) = match payload.get("role").and_then(Value::as_str) {
            Some("user") => (ConversationRole::User, "input_text"),
            Some("assistant") => (ConversationRole::Assistant, "output_text"),
            _ => continue,
        };
        if let Some(text) = codex_message_text(payload, content_type).and_then(|text| {
            if role == ConversationRole::User {
                displayable_codex_user_text(&text)
            } else {
                Some(text)
            }
        }) {
            messages.push(ConversationMessage {
                role,
                text,
                timestamp: string_field(&value, "timestamp"),
            });
        }
    }
    Ok(messages)
}

fn codex_message_text(message: &Value, content_type: &str) -> Option<String> {
    let parts = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some(content_type))
        .filter_map(|item| string_field(item, "text"))
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn displayable_codex_user_text(raw: &str) -> Option<String> {
    const INTERNAL_CONTEXT_TAGS: [&str; 4] = [
        "recommended_plugins",
        "environment_context",
        "user_action",
        "codex_internal_context",
    ];
    let mut display = raw.trim();
    loop {
        let mut removed = false;
        for tag in INTERNAL_CONTEXT_TAGS {
            let opening = format!("<{tag}>");
            let opening_with_attributes = format!("<{tag} ");
            let closing = format!("</{tag}>");
            if (display.starts_with(&opening) || display.starts_with(&opening_with_attributes))
                && let Some(end) = display.find(&closing)
            {
                display = display[end + closing.len()..].trim_start();
                removed = true;
                break;
            }
        }
        if !removed
            && display.starts_with("<image ")
            && let Some(end) = display.find('>')
        {
            display = display[end + 1..].trim_start();
            display = display
                .strip_prefix("</image>")
                .unwrap_or(display)
                .trim_start();
            removed = true;
        }
        if !removed {
            break;
        }
    }
    const INTERNAL_PREFIXES: [&str; 7] = [
        "# AGENTS.md instructions for ",
        "<app-context>",
        "<permissions instructions>",
        "<collaboration_mode>",
        "<apps_instructions>",
        "<plugins_instructions>",
        "<skills_instructions>",
    ];
    if INTERNAL_PREFIXES
        .iter()
        .any(|prefix| display.starts_with(prefix))
    {
        return None;
    }
    (!display.is_empty()).then(|| display.to_owned())
}

fn read_session_file(
    path: &Path,
    now: SystemTime,
) -> Result<(SessionAccumulator, SystemTime), ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Read {
        path: path.to_owned(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| ScanError::Metadata {
        path: path.to_owned(),
        source,
    })?;
    let fallback_updated = metadata.modified().unwrap_or(now);
    let mut accumulator = SessionAccumulator::default();

    for line in BufReader::new(file).lines() {
        match line {
            Ok(line) => {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    accumulator.consume(value);
                }
            }
            Err(source) => {
                return Err(ScanError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        }
    }

    Ok((accumulator, fallback_updated))
}

fn scan_subagents(root: &Path, now: SystemTime) -> SubagentScan {
    if !root.exists() {
        return SubagentScan::default();
    }

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            return SubagentScan {
                skipped_files: 1,
                warnings: vec![format!("Could not inspect {}: {error}", root.display())],
                ..Default::default()
            };
        }
    };
    let mut parsed = Vec::new();
    let mut skipped_files = 0;
    let mut warnings = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                skipped_files += 1;
                warnings.push(format!(
                    "Could not inspect a subagent entry under {}: {error}",
                    root.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.extension() != Some(OsStr::new("jsonl"))
            || !entry.file_name().to_string_lossy().starts_with("agent-")
        {
            continue;
        }
        match parse_subagent_file(&path, now) {
            Ok(subagent) => parsed.push(subagent),
            Err(error) => {
                skipped_files += 1;
                warnings.push(error.to_string());
            }
        }
    }

    let parent_by_tool: HashMap<String, String> = parsed
        .iter()
        .flat_map(|parent| {
            parent
                .spawned_tool_ids
                .iter()
                .cloned()
                .map(|tool_use_id| (tool_use_id, parent.model.id.clone()))
        })
        .collect();
    for subagent in &mut parsed {
        subagent.model.parent_agent_id = subagent
            .tool_use_id
            .as_ref()
            .and_then(|tool_use_id| parent_by_tool.get(tool_use_id))
            .cloned();
    }

    let mut subagents: Vec<_> = parsed.into_iter().map(|subagent| subagent.model).collect();
    subagents.sort_by(|left, right| {
        left.spawn_depth
            .cmp(&right.spawn_depth)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });

    SubagentScan {
        subagents,
        skipped_files,
        warnings,
    }
}

fn parse_subagent_file(path: &Path, now: SystemTime) -> Result<ParsedSubagent, ScanError> {
    let (accumulator, fallback_updated) = read_session_file(path, now)?;
    let metadata_path = path.with_extension("meta.json");
    let metadata = match fs::read(&metadata_path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(source) => {
            return Err(ScanError::Read {
                path: metadata_path,
                source,
            });
        }
    };
    let fallback_id = path
        .file_stem()
        .map(|stem| {
            stem.to_string_lossy()
                .trim_start_matches("agent-")
                .to_owned()
        })
        .unwrap_or_else(|| "unknown".into());
    let id = accumulator.agent_id.clone().unwrap_or(fallback_id);
    let updated = accumulator
        .last_timestamp
        .unwrap_or_else(|| DateTime::<Utc>::from(fallback_updated));
    let elapsed = now
        .duration_since(SystemTime::from(updated))
        .unwrap_or(Duration::ZERO);
    let description = string_field(&metadata, "description")
        .or_else(|| accumulator.first_prompt.clone())
        .or_else(|| accumulator.slug.clone())
        .map(|description| compact_text(&description, 88))
        .filter(|description| !description.is_empty())
        .unwrap_or_else(|| format!("Subagent {}", compact_text(&id, 12)));
    let agent_type = string_field(&metadata, "agentType")
        .or_else(|| accumulator.entrypoint.clone())
        .unwrap_or_else(|| "Subagent".into());
    let spawn_depth = metadata
        .get("spawnDepth")
        .and_then(Value::as_u64)
        .and_then(|depth| usize::try_from(depth).ok())
        .unwrap_or(1)
        .max(1);

    Ok(ParsedSubagent {
        model: CodingSubagent {
            id,
            parent_agent_id: None,
            agent_type,
            description,
            status: classify_status(elapsed, accumulator.last_turn_finished),
            updated_at: format_timestamp(updated),
            message_count: accumulator.message_count,
            last_tool: accumulator.last_tool,
            spawn_depth,
        },
        tool_use_id: string_field(&metadata, "toolUseId"),
        spawned_tool_ids: accumulator.spawned_tool_ids,
    })
}

fn classify_status(elapsed: Duration, last_turn_finished: bool) -> SessionStatus {
    if elapsed <= ACTIVE_WINDOW && !last_turn_finished {
        SessionStatus::Active
    } else if elapsed <= WAITING_WINDOW && last_turn_finished {
        SessionStatus::Waiting
    } else {
        SessionStatus::Idle
    }
}

fn extract_user_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(raw) = content.as_str() {
        return displayable_user_text(raw, PREVIEW_LIMIT);
    }

    content
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| string_field(item, "text"))
        .find_map(|raw| displayable_user_text(&raw, PREVIEW_LIMIT))
}

fn extract_full_user_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(raw) = content.as_str() {
        return displayable_full_user_text(raw);
    }
    let parts = content
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| string_field(item, "text"))
        .filter_map(|raw| displayable_full_user_text(&raw))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn extract_full_assistant_text(message: &Value) -> Option<String> {
    let parts = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| string_field(item, "text"))
        .map(|raw| normalize_claude_command_markup(&raw).trim().to_owned())
        .filter(|raw| !raw.is_empty())
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn displayable_user_text(raw: &str, limit: usize) -> Option<String> {
    displayable_full_user_text(raw).map(|text| compact_text(&text, limit))
}

fn displayable_full_user_text(raw: &str) -> Option<String> {
    const INTERNAL_PREFIXES: [&str; 6] = [
        "<local-command-caveat>",
        "<local-command-stdout>",
        "<local-command-stderr>",
        "<system-reminder>",
        "<ide_opened_file>",
        "<ide_selection>",
    ];
    let trimmed = raw.trim_start();
    if INTERNAL_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return None;
    }
    let display = normalize_claude_command_markup(raw).trim().to_owned();
    (!display.is_empty()).then_some(display)
}

fn tool_detail(name: &str, input: Option<&Value>) -> Option<String> {
    let input = input?;
    match name {
        "Read" | "Edit" | "Write" | "NotebookEdit" => string_field(input, "file_path")
            .or_else(|| string_field(input, "notebook_path"))
            .map(|path| format!("Working on {}", compact_path(&path))),
        "Bash" => string_field(input, "description")
            .map(|detail| compact_text(&detail, 100))
            .or_else(|| Some("Running a shell command".into())),
        "Grep" | "Glob" => string_field(input, "pattern")
            .map(|pattern| format!("Searching for {}", compact_text(&pattern, 80))),
        "Task" => string_field(input, "description").map(|detail| compact_text(&detail, 100)),
        _ => None,
    }
}

fn compact_path(raw: &str) -> String {
    let components: Vec<_> = Path::new(raw)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    components
        .iter()
        .skip(components.len().saturating_sub(3))
        .cloned()
        .collect::<Vec<_>>()
        .join("/")
}

fn inferred_project_path(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown project".into())
}

fn compact_text(raw: &str, limit: usize) -> String {
    let display_text = normalize_claude_command_markup(raw);
    let compact = display_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut characters = compact.chars();
    let shortened: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn normalize_claude_command_markup(raw: &str) -> String {
    let command_name = xml_tag_text(raw, "command-name").or_else(|| {
        xml_tag_text(raw, "command-message").map(|name| {
            if name.starts_with('/') {
                name
            } else {
                format!("/{name}")
            }
        })
    });
    let Some(command_name) = command_name else {
        return raw.to_owned();
    };
    let arguments = xml_tag_text(raw, "command-args").unwrap_or_default();
    if arguments.trim().is_empty() {
        command_name.trim().to_owned()
    } else {
        format!("{} {}", command_name.trim(), arguments.trim())
    }
}

fn xml_tag_text(raw: &str, tag: &str) -> Option<String> {
    let opening = format!("<{tag}>");
    let closing = format!("</{tag}>");
    let start = raw.find(&opening)? + opening.len();
    let end = raw[start..].find(&closing)? + start;
    Some(raw[start..end].trim().to_owned())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
}

fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_a_session_and_builds_a_nested_subagent_tree() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("-Users-mai-workspace-grove");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("session-1.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-1\",\"timestamp\":\"2026-07-30T00:00:00Z\",\"cwd\":\"/Users/mai/workspace/grove\",\"gitBranch\":\"main\",\"message\":{\"role\":\"user\",\"content\":\"Build the viewer\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-1\",\"timestamp\":\"2026-07-30T00:00:10Z\",\"cwd\":\"/Users/mai/workspace/grove\",\"slug\":\"green-leaf\",\"message\":{\"role\":\"assistant\",\"stop_reason\":\"tool_use\",\"content\":[{\"type\":\"tool_use\",\"id\":\"tool-parent\",\"name\":\"Agent\",\"input\":{\"description\":\"Map sessions\"}},{\"type\":\"tool_use\",\"name\":\"Read\",\"input\":{\"file_path\":\"/Users/mai/workspace/grove/src/App.tsx\"}}]}}\n",
                "{\"type\":\"ai-title\",\"sessionId\":\"session-1\",\"aiTitle\":\"Create the session viewer\"}\n"
            ),
        )
        .unwrap();
        fs::write(project.join("agent-abcd.jsonl"), "{}\n").unwrap();
        let subagents = project.join("session-1/subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(
            subagents.join("agent-parent.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-1\",\"agentId\":\"parent\",\"timestamp\":\"2026-07-30T00:00:12Z\",\"message\":{\"content\":\"Map the session graph\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-1\",\"agentId\":\"parent\",\"timestamp\":\"2026-07-30T00:00:20Z\",\"message\":{\"stop_reason\":\"tool_use\",\"content\":[{\"type\":\"tool_use\",\"id\":\"tool-child\",\"name\":\"Agent\",\"input\":{\"description\":\"Inspect nodes\"}}]}}\n"
            ),
        )
        .unwrap();
        fs::write(
            subagents.join("agent-parent.meta.json"),
            "{\"agentType\":\"Plan\",\"description\":\"Design the graph\",\"spawnDepth\":1,\"toolUseId\":\"tool-parent\"}",
        )
        .unwrap();
        fs::write(
            subagents.join("agent-child.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-1\",\"agentId\":\"child\",\"timestamp\":\"2026-07-30T00:00:21Z\",\"message\":{\"content\":\"Inspect graph nodes\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-1\",\"agentId\":\"child\",\"timestamp\":\"2026-07-30T00:00:30Z\",\"message\":{\"stop_reason\":\"end_turn\",\"content\":[{\"type\":\"text\",\"text\":\"Done\"}]}}\n"
            ),
        )
        .unwrap();
        fs::write(
            subagents.join("agent-child.meta.json"),
            "{\"agentType\":\"Explore\",\"description\":\"Inspect graph nodes\",\"spawnDepth\":2,\"toolUseId\":\"tool-child\"}",
        )
        .unwrap();

        let now = SystemTime::UNIX_EPOCH
            + Duration::from_secs(
                chrono::DateTime::parse_from_rfc3339("2026-07-30T00:00:40Z")
                    .unwrap()
                    .timestamp() as u64,
            );
        let scan = scan_claude_sessions_at(temp.path(), now).unwrap();

        assert_eq!(scan.sessions.len(), 1);
        let session = &scan.sessions[0];
        assert_eq!(session.id, "session-1");
        assert_eq!(session.title, "Create the session viewer");
        assert_eq!(session.project_name, "grove");
        assert_eq!(session.git_branch.as_deref(), Some("main"));
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.last_tool.as_deref(), Some("Read"));
        assert_eq!(session.message_count, 2);
        assert_eq!(session.subagents.len(), 2);
        assert_eq!(session.subagents[0].description, "Design the graph");
        assert_eq!(session.subagents[0].spawn_depth, 1);
        assert_eq!(
            session.subagents[1].parent_agent_id.as_deref(),
            Some("parent")
        );
        assert_eq!(session.subagents[1].spawn_depth, 2);
        assert!(scan.warnings.is_empty());
    }

    #[test]
    fn loads_complete_readable_message_history_on_demand() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("-Users-mai-workspace-grove");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("session-history.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-history\",\"timestamp\":\"2026-07-30T00:00:00Z\",\"message\":{\"content\":\"First prompt\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-history\",\"timestamp\":\"2026-07-30T00:00:10Z\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"First answer\"},{\"type\":\"tool_use\",\"name\":\"Read\"}]}}\n",
                "{\"type\":\"user\",\"sessionId\":\"session-history\",\"timestamp\":\"2026-07-30T00:00:20Z\",\"message\":{\"content\":\"<system-reminder>hidden</system-reminder>\"}}\n",
                "not-json\n"
            ),
        )
        .unwrap();

        let messages = load_session_messages_at(temp.path(), "session-history").unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ConversationRole::User);
        assert_eq!(messages[0].text, "First prompt");
        assert_eq!(messages[1].role, ConversationRole::Assistant);
        assert_eq!(messages[1].text, "First answer");
        assert_eq!(
            messages[1].timestamp.as_deref(),
            Some("2026-07-30T00:00:10Z")
        );
    }

    #[test]
    fn parses_codex_sessions_and_loads_message_history() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = temp.path().join("sessions/2026/08/02");
        fs::create_dir_all(&sessions).unwrap();
        let transcript =
            sessions.join("rollout-2026-08-02T00-00-00-019c0000-0000-7000-8000-000000000001.jsonl");
        fs::write(
            &transcript,
            concat!(
                "{\"timestamp\":\"2026-08-02T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019c0000-0000-7000-8000-000000000001\",\"cwd\":\"/Users/mai/workspace/grove\",\"source\":\"cli\",\"git\":{\"branch\":\"feat/codex\"}}}\n",
                "{\"timestamp\":\"2026-08-02T00:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n",
                "{\"timestamp\":\"2026-08-02T00:00:02Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Add Codex support\"}]}}\n",
                "{\"timestamp\":\"2026-08-02T00:00:03Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"exec_command\"}}\n",
                "{\"timestamp\":\"2026-08-02T00:00:04Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Implemented the adapter.\"}]}}\n",
                "{\"timestamp\":\"2026-08-02T00:00:05Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n"
            ),
        )
        .unwrap();
        let now = SystemTime::UNIX_EPOCH
            + Duration::from_secs(
                chrono::DateTime::parse_from_rfc3339("2026-08-02T00:00:30Z")
                    .unwrap()
                    .timestamp() as u64,
            );

        let scan = scan_codex_sessions_at(temp.path(), now).unwrap();

        assert_eq!(scan.sessions.len(), 1);
        let session = &scan.sessions[0];
        assert_eq!(session.provider, CodingAgent::Codex);
        assert_eq!(session.title, "Add Codex support");
        assert_eq!(session.project_name, "grove");
        assert_eq!(session.git_branch.as_deref(), Some("feat/codex"));
        assert_eq!(session.status, SessionStatus::Waiting);
        assert_eq!(session.last_tool.as_deref(), Some("exec_command"));
        assert_eq!(session.message_count, 2);
        assert_eq!(session.key(), "codex:019c0000-0000-7000-8000-000000000001");

        let messages =
            load_codex_session_messages_at(temp.path(), "019c0000-0000-7000-8000-000000000001")
                .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ConversationRole::User);
        assert_eq!(messages[0].text, "Add Codex support");
        assert_eq!(messages[1].role, ConversationRole::Assistant);
        assert_eq!(messages[1].text, "Implemented the adapter.");
    }

    #[test]
    fn excludes_codex_internal_subagent_rollouts_from_top_level_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = temp.path().join("sessions/2026/08/02");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(
            sessions.join("rollout-subagent.jsonl"),
            "{\"timestamp\":\"2026-08-02T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"subagent-id\",\"cwd\":\"/tmp/grove\",\"source\":{\"subagent\":\"review\"}}}\n",
        )
        .unwrap();

        let scan = scan_codex_sessions_at(temp.path(), SystemTime::now()).unwrap();

        assert!(scan.sessions.is_empty());
        assert_eq!(scan.skipped_files, 0);
    }

    #[test]
    fn strips_codex_injected_context_without_hiding_the_user_prompt() {
        let raw = concat!(
            "<recommended_plugins>plugin catalog</recommended_plugins>",
            "<environment_context>local metadata</environment_context>",
            "Please add a provider filter."
        );

        assert_eq!(
            displayable_codex_user_text(raw).as_deref(),
            Some("Please add a provider filter.")
        );
        assert_eq!(
            displayable_codex_user_text(
                "<user_action><context>internal action</context></user_action>"
            ),
            None
        );
        assert_eq!(
            displayable_codex_user_text("# AGENTS.md instructions for /Users/mai/workspace"),
            None
        );
        assert_eq!(
            displayable_codex_user_text(concat!(
                "<codex_internal_context source=\"desktop\">hidden</codex_internal_context>",
                "<image name=\"reference\" path=\"/private/reference.png\">",
                "Describe the screenshot."
            ))
            .as_deref(),
            Some("Describe the screenshot.")
        );
    }

    #[test]
    fn classifies_finished_and_stale_sessions() {
        assert_eq!(
            classify_status(Duration::from_secs(30), true),
            SessionStatus::Waiting
        );
        assert_eq!(
            classify_status(Duration::from_secs(1_000), true),
            SessionStatus::Idle
        );
        assert_eq!(
            classify_status(Duration::from_secs(20), false),
            SessionStatus::Active
        );
    }

    #[test]
    fn a_new_prompt_after_a_finished_turn_is_active_again() {
        let mut session = SessionAccumulator::default();
        session.consume(serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-07-30T00:00:00Z",
            "message": {
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "Done"}]
            }
        }));
        assert!(session.last_turn_finished);

        session.consume(serde_json::json!({
            "type": "user",
            "timestamp": "2026-07-30T00:01:00Z",
            "message": {"content": "One more change"}
        }));
        assert!(!session.last_turn_finished);
    }

    #[test]
    fn compacts_unicode_by_character_not_byte() {
        assert_eq!(compact_text("日本語のsession", 4), "日本語の…");
    }

    #[test]
    fn renders_claude_slash_command_markup_as_plain_text() {
        let raw = concat!(
            "<command-message>review</command-message> ",
            "<command-name>/review</command-name> ",
            "<command-args>#17 verify the patch</command-args>"
        );

        assert_eq!(compact_text(raw, 96), "/review #17 verify the patch");
        assert_eq!(
            compact_text("<command-message>compact</command-message>", 96),
            "/compact"
        );
    }

    #[test]
    fn excludes_claude_internal_messages_from_user_prompts() {
        let caveat = serde_json::json!({
            "content": "<local-command-caveat>Internal instructions</local-command-caveat>"
        });
        let mixed = serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": "<system-reminder>Internal context</system-reminder>"
                },
                {"type": "text", "text": "Actual user prompt"}
            ]
        });

        assert_eq!(extract_user_text(&caveat), None);
        assert_eq!(
            extract_user_text(&mixed).as_deref(),
            Some("Actual user prompt")
        );
    }

    #[test]
    fn reports_unreadable_session_contents_as_a_partial_scan() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("-Users-mai-workspace-grove");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("broken.jsonl"), [0xff, b'\n']).unwrap();

        let scan = scan_claude_sessions_at(temp.path(), SystemTime::now()).unwrap();

        assert!(scan.sessions.is_empty());
        assert_eq!(scan.skipped_files, 1);
        assert_eq!(scan.warnings.len(), 1);
        assert!(scan.warnings[0].contains("failed to read"));
    }

    #[test]
    #[ignore = "requires a local Claude Code installation with session history"]
    fn scans_installed_claude_sessions() {
        let root = dirs::home_dir()
            .expect("home directory")
            .join(".claude")
            .join("projects");
        let started = std::time::Instant::now();
        let scan = scan_claude_sessions_at(&root, SystemTime::now()).unwrap();

        assert!(
            !scan.sessions.is_empty(),
            "expected at least one local Claude Code session"
        );
        let subagent_count: usize = scan
            .sessions
            .iter()
            .map(|session| session.subagents.len())
            .sum();
        eprintln!(
            "scanned {} sessions and {} subagents ({} skipped) in {:?}",
            scan.sessions.len(),
            subagent_count,
            scan.skipped_files,
            started.elapsed()
        );
    }

    #[test]
    #[ignore = "requires a local Codex installation with session history"]
    fn scans_installed_codex_sessions() {
        let root = dirs::home_dir().expect("home directory").join(".codex");
        let started = std::time::Instant::now();
        let scan = scan_codex_sessions_at(&root, SystemTime::now()).unwrap();

        assert!(
            !scan.sessions.is_empty(),
            "expected at least one local Codex session"
        );
        eprintln!(
            "scanned {} Codex sessions ({} skipped) in {:?}",
            scan.sessions.len(),
            scan.skipped_files,
            started.elapsed()
        );
    }
}
