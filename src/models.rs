use serde::Serialize;

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
pub struct ClaudeSubagent {
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
pub struct ClaudeSession {
    pub id: String,
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
    pub subagents: Vec<ClaudeSubagent>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionScan {
    pub sessions: Vec<ClaudeSession>,
    pub scanned_at: String,
    pub source_root: String,
    pub skipped_files: usize,
    pub warnings: Vec<String>,
}
