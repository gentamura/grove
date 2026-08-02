use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum CodingAgent {
    ClaudeCode,
    Codex,
}

impl CodingAgent {
    pub const fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn resume_command(self, session_id: &str) -> String {
        match self {
            Self::ClaudeCode => format!("claude --resume {session_id}"),
            Self::Codex => format!("codex resume {session_id}"),
        }
    }

    pub fn session_key(self, session_id: &str) -> String {
        match self {
            Self::ClaudeCode => session_id.to_owned(),
            Self::Codex => format!("{}:{session_id}", self.key()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivity {
    pub kind: ActivityKind,
    pub label: String,
    pub detail: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Prompt,
    Response,
    Tool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Waiting,
    Idle,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodingSubagent {
    pub id: String,
    pub parent_agent_id: Option<String>,
    pub agent_type: String,
    pub description: String,
    pub status: SessionStatus,
    pub updated_at: String,
    pub message_count: usize,
    pub last_tool: Option<String>,
    pub spawn_depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodingSession {
    pub id: String,
    pub provider: CodingAgent,
    pub title: String,
    pub project_name: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub slug: Option<String>,
    pub status: SessionStatus,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub message_count: usize,
    pub last_prompt: Option<String>,
    pub last_tool: Option<String>,
    pub activities: Vec<SessionActivity>,
    pub subagents: Vec<CodingSubagent>,
}

impl CodingSession {
    pub fn key(&self) -> String {
        self.provider.session_key(&self.id)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionScan {
    pub sessions: Vec<CodingSession>,
    pub scanned_at: String,
    pub source_roots: Vec<String>,
    pub skipped_files: usize,
    pub warnings: Vec<String>,
}
