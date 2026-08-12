//! Core protocol types shared across Stratum crates.

pub mod agent_loop_event;
pub mod error;

use std::{fmt, str::FromStr};

use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use stratum_macros::{sha256_fingerprint, string_id, uuid_identity};
use utoipa::ToSchema;

pub use agent_loop_event::{
    AfterToolCallDecisionRecord, AgentTelemetryEvent, AuthorizationOverrideRecord, ContextPatch,
    DecideToolCallDecisionRecord, DurableAgentEvent, HookDecisionRecord,
    PrepareNextTurnDecisionRecord, TransformContextDecisionRecord, TransformToolCallDecisionRecord,
    TransformToolCallModificationRecord,
};
pub use error::{AgentVersionTagParseError, FingerprintParseError, HookFailure, ModelIdParseError};

uuid_identity!(SessionId, "Identity of one long-lived runtime session.");
uuid_identity!(
    TurnId,
    "Identity of one resumable turn inside a workflow run."
);
uuid_identity!(AgentId, "Identity of one immutable Agent template version.");
uuid_identity!(
    AgentRuntimeId,
    "Identity of one long-lived Agent runtime aggregate."
);
uuid_identity!(
    WorkflowVersionId,
    "Identity of one immutable published workflow version."
);
uuid_identity!(
    SkillSetVersionId,
    "Identity of one immutable ordered skill set version."
);
uuid_identity!(
    ExtensionSetVersionId,
    "Identity of one immutable ordered extension set version."
);
uuid_identity!(
    HookHandlerVersionId,
    "Identity of one immutable hook handler version."
);
uuid_identity!(
    HookInvocationId,
    "Identity of one hook handler invocation at one hook point."
);
uuid_identity!(ApprovalId, "Identity of one tool approval request.");

/// Whether a tool observes or mutates state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolKind {
    /// Tool only observes state.
    Read,
    /// Tool may mutate state.
    Write,
}

/// Declared danger of one tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DangerLevel {
    /// Low danger.
    Low,
    /// Medium danger.
    Medium,
    /// High danger.
    High,
}

/// User decision for one tool approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalDecision {
    /// Approve the tool call.
    Approve,
    /// Reject the tool call.
    Reject,
}

string_id!(NodeId, "Identity of a workflow node.");
string_id!(CallId, "Identity of one tool call.");
string_id!(ToolName, "Provider-visible identity of a tool.");
string_id!(LlmCallId, "Identity of one LLM call.");
string_id!(PlanId, "Identity of an agent-visible plan.");

/// Author-supplied identity tag for one immutable Agent template version.
///
/// Tags are compared byte-for-byte. They are case-sensitive and intentionally
/// have no ordering, normalization, or semantic-version meaning.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(try_from = "String", into = "String")]
pub struct AgentVersionTag(String);

impl AgentVersionTag {
    /// Parses an author-supplied Agent template version tag.
    ///
    /// # Errors
    ///
    /// Returns [`AgentVersionTagParseError`] when `value` is empty, longer
    /// than 128 UTF-8 bytes, contains a control character, or has leading or
    /// trailing whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AgentVersionTagParseError> {
        value.into().try_into()
    }

    /// Returns the original, unnormalized tag.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentVersionTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for AgentVersionTag {
    type Err = AgentVersionTagParseError;

    /// Parses an author-supplied Agent template version tag.
    ///
    /// # Errors
    ///
    /// Returns [`AgentVersionTagParseError`] when the tag is outside the
    /// durable protocol boundary.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for AgentVersionTag {
    type Error = AgentVersionTagParseError;

    /// Validates an owned Agent template version tag without normalizing it.
    ///
    /// # Errors
    ///
    /// Returns [`AgentVersionTagParseError`] when the tag is outside the
    /// durable protocol boundary.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(AgentVersionTagParseError::Empty);
        }
        if value.len() > 128 {
            return Err(AgentVersionTagParseError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(AgentVersionTagParseError::ControlCharacter);
        }
        if value.trim() != value {
            return Err(AgentVersionTagParseError::SurroundingWhitespace);
        }

        Ok(Self(value))
    }
}

impl From<AgentVersionTag> for String {
    fn from(value: AgentVersionTag) -> Self {
        value.0
    }
}

sha256_fingerprint!(
    ToolSetFingerprint,
    "SHA-256 fingerprint of an exact ordered runtime tool set.",
    FingerprintParseError
);
sha256_fingerprint!(
    HookInputDigest,
    "SHA-256 digest of the canonical input to one hook handler invocation.",
    FingerprintParseError
);

/// Canonical identity of a provider model.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema)]
#[serde(try_from = "String", into = "String")]
pub struct ModelId(String);

impl ModelId {
    /// Creates a canonical provider-scoped model id.
    ///
    /// # Errors
    ///
    /// Returns [`ModelIdParseError::InvalidFormat`] when either segment is invalid.
    pub fn new(provider: &str, model: &str) -> Result<Self, ModelIdParseError> {
        format!("{provider}:{model}").parse()
    }

    /// Returns the canonical provider name.
    #[must_use]
    pub fn provider_name(&self) -> &str {
        // Invariant: `ModelId` is only constructible through the validating
        // `FromStr`, which guarantees the `provider:model` separator exists.
        self.0.split_once(':').expect("validated model id").0
    }

    /// Returns the provider-local model name.
    #[must_use]
    pub fn model_name(&self) -> &str {
        // Invariant: `ModelId` is only constructible through the validating
        // `FromStr`, which guarantees the `provider:model` separator exists.
        self.0.split_once(':').expect("validated model id").1
    }

    /// Returns the canonical model id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ModelId {
    type Err = ModelIdParseError;

    /// Parses a canonical `provider:model` id.
    ///
    /// # Errors
    ///
    /// Returns [`ModelIdParseError::InvalidFormat`] when the value is not canonical.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((provider, model)) = value.split_once(':') else {
            return Err(ModelIdParseError::InvalidFormat);
        };
        if provider.is_empty()
            || model.is_empty()
            || model.contains(':')
            || provider.chars().any(char::is_whitespace)
            || model.chars().any(char::is_whitespace)
        {
            return Err(ModelIdParseError::InvalidFormat);
        }

        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for ModelId {
    type Error = ModelIdParseError;

    /// Converts a canonical `provider:model` id.
    ///
    /// # Errors
    ///
    /// Returns [`ModelIdParseError::InvalidFormat`] when the value is not canonical.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ModelId> for String {
    fn from(value: ModelId) -> Self {
        value.0
    }
}

/// Stable model selection and provider parameters for an agent turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// Canonical provider-scoped model identity.
    pub model: ModelId,
    /// Provider-specific model parameters.
    pub parameters: Map<String, Value>,
}

impl ModelConfig {
    /// Creates a stable model configuration.
    #[must_use]
    pub fn new(model: ModelId, parameters: Map<String, Value>) -> Self {
        Self { model, parameters }
    }
}

/// Location where an agent is executing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum AgentLocation {
    /// Agent is executing directly in a session.
    Direct,
    /// Agent is embedded as one workflow node.
    WorkflowNode {
        /// Immutable workflow version containing the node.
        workflow_version_id: WorkflowVersionId,
        /// Node containing the agent.
        node_id: NodeId,
    },
}

/// Immutable host-supplied context for one agent turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AgentRuntimeContext {
    /// Long-lived session containing the turn.
    pub session_id: SessionId,
    /// Location where the agent is executing.
    pub location: AgentLocation,
}

impl AgentRuntimeContext {
    /// Creates an Agent runtime context from a validated session and location.
    #[must_use]
    pub const fn new(session_id: SessionId, location: AgentLocation) -> Self {
        Self {
            session_id,
            location,
        }
    }

    /// Creates direct agent runtime context for a session.
    #[must_use]
    pub const fn direct(session_id: SessionId) -> Self {
        Self {
            session_id,
            location: AgentLocation::Direct,
        }
    }

    /// Creates workflow-node runtime context for a session.
    #[must_use]
    pub const fn workflow_node(
        session_id: SessionId,
        workflow_version_id: WorkflowVersionId,
        node_id: NodeId,
    ) -> Self {
        Self {
            session_id,
            location: AgentLocation::WorkflowNode {
                workflow_version_id,
                node_id,
            },
        }
    }
}

/// Exact runtime components pinned for one resumable turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TurnRuntimeSnapshot {
    /// Immutable agent definition used by the turn.
    pub agent_id: AgentId,
    /// Fully resolved model configuration used by the turn.
    pub model: ModelConfig,
    /// Exact ordered set of tools visible to the model.
    pub tool_set_fingerprint: ToolSetFingerprint,
    /// Immutable ordered skill set used by the turn.
    pub skill_set_version_id: SkillSetVersionId,
    /// Immutable ordered extension set used by the turn.
    pub extension_set_version_id: ExtensionSetVersionId,
    /// Exact hook handler order resolved for the turn.
    pub hook_handler_versions: Vec<HookHandlerVersionId>,
}

impl TurnRuntimeSnapshot {
    /// Creates an exact runtime snapshot for one resumable Turn.
    #[must_use]
    pub fn new(
        agent_id: AgentId,
        model: ModelConfig,
        tool_set_fingerprint: ToolSetFingerprint,
        skill_set_version_id: SkillSetVersionId,
        extension_set_version_id: ExtensionSetVersionId,
        hook_handler_versions: Vec<HookHandlerVersionId>,
    ) -> Self {
        Self {
            agent_id,
            model,
            tool_set_fingerprint,
            skill_set_version_id,
            extension_set_version_id,
            hook_handler_versions,
        }
    }
}

/// Decision-affecting point in the agent loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookPoint {
    /// Transform model context before a model call.
    TransformContext,
    /// Transform tool call arguments before the execution decision.
    TransformToolCall,
    /// Decide whether a validated tool call executes.
    DecideToolCall,
    /// Process a completed tool call before the loop continues.
    AfterToolCall,
    /// Prepare state for the next model iteration.
    PrepareNextTurn,
}

/// Operation inside a turn to which a hook invocation belongs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum HookOperationIdentity {
    /// Operation applies to the turn as a whole.
    Turn,
    /// Operation applies to one model-loop iteration.
    Iteration {
        /// Zero-based iteration index.
        index: u32,
    },
    /// Operation applies to one tool call.
    ToolCall {
        /// Provider tool-call identity.
        call_id: CallId,
    },
}

/// Stable semantic address of one handler invocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct HookInvocationAddress {
    /// Long-lived session containing the invocation.
    pub session_id: SessionId,
    /// Agent whose loop invokes the handler.
    pub agent_id: AgentId,
    /// Resumable turn containing the invocation.
    pub turn_id: TurnId,
    /// Hook point being handled.
    pub hook_point: HookPoint,
    /// Zero-based position in the pinned handler chain.
    pub handler_position: u32,
    /// Immutable handler version at this position.
    pub handler_version_id: HookHandlerVersionId,
    /// Operation affected by the handler decision.
    pub operation: HookOperationIdentity,
}

impl HookInvocationAddress {
    fn has_same_semantics_except_version(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.agent_id == other.agent_id
            && self.turn_id == other.turn_id
            && self.hook_point == other.hook_point
            && self.handler_position == other.handler_position
            && self.operation == other.operation
    }
}

/// Persisted lifecycle of one decision-affecting hook invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookInvocationState<T> {
    /// Invocation identity and input were committed before calling the handler.
    Pending,
    /// A validated decision was committed before applying it.
    Completed {
        /// Typed handler decision.
        decision: T,
    },
    /// Handler invocation reached a typed terminal failure.
    Failed {
        /// Safe failure classification without sensitive handler payloads.
        failure: HookFailure,
    },
    /// Handler invocation reached its deadline.
    TimedOut,
    /// Handler invocation was cancelled.
    Cancelled,
}

/// Journal-independent record required to resume one hook invocation safely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct HookInvocationRecord<T> {
    /// Stable remote idempotency key for this handler call.
    pub invocation_id: HookInvocationId,
    /// Semantic address that prevents duplicate logical invocations.
    pub address: HookInvocationAddress,
    /// Digest of the canonical handler input.
    pub input_digest: HookInputDigest,
    /// Persisted invocation lifecycle.
    pub state: HookInvocationState<T>,
}

impl<T> HookInvocationRecord<T> {
    /// Creates a pending record that must be committed before invoking a handler.
    #[must_use]
    pub fn pending(address: HookInvocationAddress, input_digest: HookInputDigest) -> Self {
        Self {
            invocation_id: HookInvocationId::new(),
            address,
            input_digest,
            state: HookInvocationState::Pending,
        }
    }

    /// Validates a resumed invocation and chooses retry, reuse, or terminal failure.
    ///
    /// # Errors
    ///
    /// Returns a typed, fail-closed [`HookFailure`] for any address, version, input,
    /// or persisted terminal-state mismatch.
    pub fn resume<'a>(
        &'a self,
        address: &HookInvocationAddress,
        input_digest: &HookInputDigest,
    ) -> Result<HookResume<'a, T>, HookFailure> {
        if !self.address.has_same_semantics_except_version(address) {
            return Err(HookFailure::AddressMismatch);
        }
        if self.address.handler_version_id != address.handler_version_id {
            return Err(HookFailure::VersionMismatch);
        }
        if &self.input_digest != input_digest {
            return Err(HookFailure::InputMismatch);
        }

        match &self.state {
            HookInvocationState::Pending => Ok(HookResume::Retry {
                invocation_id: self.invocation_id,
            }),
            HookInvocationState::Completed { decision } => Ok(HookResume::Reuse {
                invocation_id: self.invocation_id,
                decision,
            }),
            HookInvocationState::Failed { failure } => Err(*failure),
            HookInvocationState::TimedOut => Err(HookFailure::TimedOut),
            HookInvocationState::Cancelled => Err(HookFailure::Cancelled),
        }
    }
}

/// Safe action selected after validating a persisted hook invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HookResume<'a, T> {
    /// Retry a pending handler with the original idempotency key.
    Retry {
        /// Original invocation identity.
        invocation_id: HookInvocationId,
    },
    /// Reuse a committed decision without invoking the handler again.
    Reuse {
        /// Original invocation identity.
        invocation_id: HookInvocationId,
        /// Persisted typed decision.
        decision: &'a T,
    },
}

/// Supported extension packaging forms and their trust boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExtensionForm {
    /// Immutable skill content treated as untrusted data.
    Skill,
    /// Untrusted executable script isolated from the agent process.
    Script,
    /// Fully trusted hook linked into the agent process.
    LinkedRust,
    /// Trusted remote service reached through an untrusted transport.
    HookService,
}

/// Executable minimum boundary associated with one extension form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtensionBoundary {
    /// Whether loading the extension may add Tool or secret permissions.
    pub may_elevate_permissions: bool,
    /// Whether code must execute outside the agent process.
    pub requires_process_isolation: bool,
    /// Whether time, memory, output, and concurrency limits are required.
    pub requires_resource_limits: bool,
    /// Whether runtime compatibility must be pinned for resume.
    pub requires_runtime_compatibility_pin: bool,
    /// Whether authenticated and authorized transport is required.
    pub requires_authenticated_transport: bool,
    /// Whether remote service identity, version, and endpoint must be pinned.
    pub requires_service_identity_pin: bool,
    /// Whether the invocation identity must be accepted as an idempotency key.
    pub requires_invocation_idempotency: bool,
    /// Whether logs and public errors must omit sensitive hook payloads.
    pub redact_sensitive_payloads: bool,
}

impl ExtensionForm {
    /// Returns the minimum enforceable trust boundary for this extension form.
    #[must_use]
    pub const fn boundary(self) -> ExtensionBoundary {
        match self {
            Self::Skill => ExtensionBoundary {
                may_elevate_permissions: false,
                requires_process_isolation: false,
                requires_resource_limits: false,
                requires_runtime_compatibility_pin: false,
                requires_authenticated_transport: false,
                requires_service_identity_pin: false,
                requires_invocation_idempotency: false,
                redact_sensitive_payloads: true,
            },
            Self::Script => ExtensionBoundary {
                may_elevate_permissions: false,
                requires_process_isolation: true,
                requires_resource_limits: true,
                requires_runtime_compatibility_pin: false,
                requires_authenticated_transport: false,
                requires_service_identity_pin: false,
                requires_invocation_idempotency: false,
                redact_sensitive_payloads: true,
            },
            Self::LinkedRust => ExtensionBoundary {
                may_elevate_permissions: false,
                requires_process_isolation: false,
                requires_resource_limits: false,
                requires_runtime_compatibility_pin: true,
                requires_authenticated_transport: false,
                requires_service_identity_pin: false,
                requires_invocation_idempotency: false,
                redact_sensitive_payloads: true,
            },
            Self::HookService => ExtensionBoundary {
                may_elevate_permissions: false,
                requires_process_isolation: false,
                requires_resource_limits: false,
                requires_runtime_compatibility_pin: false,
                requires_authenticated_transport: true,
                requires_service_identity_pin: true,
                requires_invocation_idempotency: true,
                redact_sensitive_payloads: true,
            },
        }
    }
}

/// Token usage reported by a model provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TokenUsage {
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens produced.
    pub output_tokens: u64,
    /// Total tokens reported by the provider.
    pub total_tokens: u64,
}

/// Complete tool call emitted by a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolCall {
    /// Provider call identity.
    pub call_id: CallId,
    /// Provider-visible tool name.
    pub name: String,
    /// Parsed tool arguments.
    pub arguments: Value,
}

/// Incremental tool call update from a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Position of the tool call in the response.
    pub index: usize,
    /// Provider call identity when known.
    pub call_id: Option<CallId>,
    /// Provider-visible tool name when known.
    pub name: Option<String>,
    /// Raw argument text fragment.
    pub arguments_delta: String,
}

/// Role of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatRole {
    /// System instruction message.
    System,
    /// End-user message.
    User,
    /// Assistant response message.
    Assistant,
    /// Tool result message.
    Tool,
}

/// Content carried by a chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum ChatContent {
    /// Plain text content.
    Text(String),
    /// JSON content.
    Json(Value),
}

/// Message exchanged with an LLM provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ChatMessage {
    /// Message role.
    pub role: ChatRole,
    /// Message content.
    pub content: ChatContent,
    /// Tool calls requested by an assistant message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Reasoning content produced by an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Tool call this tool message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<CallId>,
}

impl ChatMessage {
    /// Creates a system text message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(ChatRole::System, content)
    }

    /// Creates a user text message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(ChatRole::User, content)
    }

    /// Creates an assistant text message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(ChatRole::Assistant, content)
    }

    /// Creates a tool result message.
    #[must_use]
    pub fn tool(call_id: impl Into<CallId>, result: Value) -> Self {
        Self {
            role: ChatRole::Tool,
            content: ChatContent::Json(result),
            tool_calls: Vec::new(),
            reasoning_content: None,
            tool_call_id: Some(call_id.into()),
        }
    }

    /// Creates a text message for a role.
    #[must_use]
    pub fn text(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: ChatContent::Text(content.into()),
            tool_calls: Vec::new(),
            reasoning_content: None,
            tool_call_id: None,
        }
    }

    /// Sets assistant reasoning content.
    #[must_use]
    pub fn with_reasoning_content(mut self, content: impl Into<String>) -> Self {
        self.reasoning_content = Some(content.into());
        self
    }

    /// Sets tool calls for this message.
    #[must_use]
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }
}

/// Tool definition exposed to an LLM provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[non_exhaustive]
pub struct ToolSpec {
    /// Provider-visible tool name.
    #[builder(into)]
    pub name: ToolName,
    /// Provider-visible tool description.
    #[builder(into)]
    pub description: String,
    /// JSON schema for tool input.
    pub input_schema: Value,
}

/// Role of one normal text delta in an LLM call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LlmCallRole {
    /// System instruction.
    System,
    /// End-user input.
    User,
    /// Assistant-visible answer.
    Assistant,
    /// Tool result output.
    Tool,
}

/// Event emitted inside one LLM call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum LlmEvent {
    /// LLM call started.
    Started,
    /// LLM call finished.
    Finished {
        /// Why the LLM call finished.
        finish_reason: String,
        /// Token usage reported by the provider.
        usage: Option<TokenUsage>,
    },
    /// LLM call failed.
    Failed {
        /// Error text safe to expose to callers.
        error_text: String,
    },
    /// LLM call emitted normal text.
    TextDelta {
        /// Role of this text fragment.
        role: LlmCallRole,
        /// Visible text delta.
        delta: String,
    },
    /// LLM call emitted reasoning text.
    ReasoningDelta {
        /// Reasoning text delta.
        delta: String,
    },
    /// Tool call started.
    ToolCallStarted {
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name.
        name: Option<String>,
    },
    /// Tool call arguments changed.
    ToolCallDelta {
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name when known.
        name: Option<String>,
        /// Tool argument text fragment.
        arguments_delta: String,
    },
    /// Tool call finished.
    ToolCallFinished {
        /// Tool call identity.
        call_id: CallId,
        /// Tool result.
        result: Value,
    },
    /// Tool call failed.
    ToolCallFailed {
        /// Tool call identity.
        call_id: CallId,
        /// Error text safe to expose to callers.
        error_text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_uses_uuid_v7() {
        let version = SessionId::new().as_uuid().get_version_num();

        assert_eq!(version, 7);
    }

    #[test]
    fn turn_id_new_uses_uuid_v7() {
        let id = TurnId::new();

        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn turn_id_round_trips_string() {
        let id = TurnId::new();
        let parsed = id.to_string().parse::<TurnId>().expect("turn id parses");

        assert_eq!(parsed, id);
    }

    #[test]
    fn agent_id_new_uses_uuid_v7() {
        let id = AgentId::new();

        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn model_id_round_trips_provider_and_model_name() {
        let model: ModelId = "openai:gpt-4.1-mini".parse().expect("model id parses");
        let constructed = ModelId::new("openai", "gpt-4.1-mini").expect("model id constructs");
        let converted =
            ModelId::try_from("openai:gpt-4.1-mini".to_owned()).expect("model id converts");

        assert_eq!(model.provider_name(), "openai");
        assert_eq!(model.model_name(), "gpt-4.1-mini");
        assert_eq!(model.as_str(), "openai:gpt-4.1-mini");
        assert_eq!(model, constructed);
        assert_eq!(model, converted);
    }

    #[test]
    fn model_config_constructs_with_model_and_parameters() {
        let model = ModelId::new("openai", "test-model").expect("static model is valid");
        let config = ModelConfig::new(model.clone(), Map::new());

        assert_eq!(config.model, model);
        assert!(config.parameters.is_empty());
    }

    #[test]
    fn model_id_rejects_noncanonical_values() {
        for value in [
            "gpt-4.1-mini",
            ":gpt",
            "openai:",
            "openai:gpt:mini",
            "open ai:gpt",
            "openai:gpt mini",
            "openai: gpt",
        ] {
            assert!(value.parse::<ModelId>().is_err(), "{value} should fail");
        }
    }

    #[test]
    fn tool_name_round_trips_string() {
        let tool_name = ToolName::from("echo");

        assert_eq!(tool_name.as_str(), "echo");
        assert_eq!(tool_name.to_string(), "echo");
    }

    #[test]
    fn chat_message_user_constructor_sets_role_and_text() {
        let message = ChatMessage::user("hello");

        assert_eq!(message.role, ChatRole::User);
        assert_eq!(message.content, ChatContent::Text("hello".to_owned()));
        assert!(message.tool_calls.is_empty());
        assert!(message.tool_call_id.is_none());
    }

    #[test]
    fn chat_message_with_tool_calls_sets_tool_calls() {
        let calls = vec![
            ToolCall {
                call_id: CallId::from("call-1"),
                name: "get_weather".to_owned(),
                arguments: serde_json::json!({"city": "Tokyo"}),
            },
            ToolCall {
                call_id: CallId::from("call-2"),
                name: "get_time".to_owned(),
                arguments: serde_json::json!({"tz": "UTC"}),
            },
        ];
        let message = ChatMessage::assistant("hi").with_tool_calls(calls.clone());

        assert_eq!(message.tool_calls, calls);
    }

    #[test]
    fn tool_message_records_answered_call_id() {
        let message = ChatMessage::tool(CallId::from("call-1"), serde_json::json!({"ok": true}));

        assert_eq!(message.role, ChatRole::Tool);
        assert_eq!(message.tool_call_id, Some(CallId::from("call-1")));
        assert_eq!(
            message.content,
            ChatContent::Json(serde_json::json!({"ok": true}))
        );
    }

    #[test]
    fn tool_spec_serializes_provider_visible_shape() {
        let spec = ToolSpec::builder()
            .name("echo")
            .description("returns input arguments")
            .input_schema(serde_json::json!({"type": "object"}))
            .build();

        assert_eq!(
            serde_json::to_value(spec).expect("tool spec should serialize"),
            serde_json::json!({
                "name": "echo",
                "description": "returns input arguments",
                "input_schema": {"type": "object"}
            })
        );
    }

    #[test]
    fn llm_call_id_round_trips_string() {
        let llm_call_id = LlmCallId::from("llm-call-1");

        assert_eq!(llm_call_id.as_str(), "llm-call-1");
        assert_eq!(llm_call_id.to_string(), "llm-call-1");
    }

    #[test]
    fn user_input_uses_text_delta_role() {
        let event = LlmEvent::TextDelta {
            role: LlmCallRole::User,
            delta: "hello".to_owned(),
        };
        let value = serde_json::to_value(event).expect("event should serialize");

        assert_eq!(value["type"], "text_delta");
        assert_eq!(value["data"]["role"], "user");
    }

    #[test]
    fn assistant_output_uses_text_delta_role() {
        let event = LlmEvent::TextDelta {
            role: LlmCallRole::Assistant,
            delta: "hello".to_owned(),
        };
        let value = serde_json::to_value(event).expect("event should serialize");

        assert_eq!(value["type"], "text_delta");
        assert_eq!(value["data"]["role"], "assistant");
    }

    #[test]
    fn reasoning_delta_is_not_a_text_role() {
        let event = LlmEvent::ReasoningDelta {
            delta: "thinking".to_owned(),
        };
        let value = serde_json::to_value(event).expect("event should serialize");

        assert_eq!(value["type"], "reasoning_delta");
    }

    #[test]
    fn tool_call_delta_supports_partial_arguments() {
        let event = LlmEvent::ToolCallDelta {
            call_id: CallId::from("call-1"),
            name: Some("get_weather".to_owned()),
            arguments_delta: "{\"city".to_owned(),
        };
        let value = serde_json::to_value(event).expect("event should serialize");

        assert_eq!(value["type"], "tool_call_delta");
        assert_eq!(value["data"]["call_id"], "call-1");
    }

    #[test]
    fn approval_id_uses_uuid_v7() {
        assert_eq!(ApprovalId::new().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn runtime_identity_newtypes_use_uuid_v7_and_reject_invalid_input() {
        macro_rules! assert_identity {
            ($identity:ty) => {{
                let id = <$identity>::new();
                assert_eq!(id.as_uuid().get_version_num(), 7);
                assert_eq!(
                    id.to_string()
                        .parse::<$identity>()
                        .expect("identity parses"),
                    id
                );
                assert!("not-a-uuid".parse::<$identity>().is_err());
                assert!(serde_json::from_str::<$identity>(r#""not-a-uuid""#).is_err());
            }};
        }

        assert_identity!(SessionId);
        assert_identity!(AgentId);
        assert_identity!(AgentRuntimeId);
        assert_identity!(WorkflowVersionId);
        assert_identity!(SkillSetVersionId);
        assert_identity!(ExtensionSetVersionId);
        assert_identity!(HookHandlerVersionId);
        assert_identity!(HookInvocationId);
    }

    #[test]
    fn agent_version_tag_preserves_author_value_and_rejects_invalid_boundaries() {
        let tag = AgentVersionTag::new("Release-α").expect("valid tag");
        assert_eq!(tag.as_str(), "Release-α");
        assert_ne!(
            tag,
            AgentVersionTag::new("release-α").expect("valid case-distinct tag")
        );

        assert_eq!(
            AgentVersionTag::new("").expect_err("empty tag is invalid"),
            AgentVersionTagParseError::Empty
        );
        assert_eq!(
            AgentVersionTag::new("x".repeat(129)).expect_err("oversized tag is invalid"),
            AgentVersionTagParseError::TooLong
        );
        assert_eq!(
            AgentVersionTag::new("release\n1").expect_err("control character is invalid"),
            AgentVersionTagParseError::ControlCharacter
        );
        assert_eq!(
            AgentVersionTag::new(" release-1").expect_err("leading whitespace is invalid"),
            AgentVersionTagParseError::SurroundingWhitespace
        );
        assert_eq!(
            AgentVersionTag::new("release-1 ").expect_err("trailing whitespace is invalid"),
            AgentVersionTagParseError::SurroundingWhitespace
        );
        assert!(serde_json::from_str::<AgentVersionTag>(r#"" release-1""#).is_err());
    }

    #[test]
    fn agent_runtime_context_preserves_direct_and_workflow_locations() {
        let session_id = SessionId::new();
        let workflow_version_id = WorkflowVersionId::new();
        let direct = AgentRuntimeContext::direct(session_id);
        let workflow = AgentRuntimeContext::workflow_node(
            session_id,
            workflow_version_id,
            NodeId::from("agent-node"),
        );

        assert_eq!(direct.location, AgentLocation::Direct);
        assert_eq!(
            workflow.location,
            AgentLocation::WorkflowNode {
                workflow_version_id,
                node_id: NodeId::from("agent-node"),
            }
        );
        assert_eq!(
            serde_json::to_value(workflow).expect("workflow context serializes")["location"]["type"],
            "workflow_node"
        );
    }

    fn hook_address(handler_position: u32) -> HookInvocationAddress {
        HookInvocationAddress {
            session_id: SessionId::new(),
            agent_id: AgentId::new(),
            turn_id: TurnId::new(),
            hook_point: HookPoint::DecideToolCall,
            handler_position,
            handler_version_id: HookHandlerVersionId::new(),
            operation: HookOperationIdentity::ToolCall {
                call_id: CallId::from("call-1"),
            },
        }
    }

    fn hook_digest(value: char) -> HookInputDigest {
        value
            .to_string()
            .repeat(64)
            .parse()
            .expect("test digest is valid")
    }

    #[test]
    fn each_handler_gets_a_distinct_hook_invocation_identity() {
        let base = hook_address(0);
        let first = HookInvocationRecord::<bool>::pending(base.clone(), hook_digest('a'));
        let mut second_address = base;
        second_address.handler_position = 1;
        second_address.handler_version_id = HookHandlerVersionId::new();
        let second = HookInvocationRecord::<bool>::pending(second_address, hook_digest('b'));

        assert_ne!(first.invocation_id, second.invocation_id);
    }

    #[test]
    fn pending_hook_resume_reuses_the_original_idempotency_key() {
        let address = hook_address(0);
        let digest = hook_digest('a');
        let record = HookInvocationRecord::<bool>::pending(address.clone(), digest.clone());

        assert_eq!(
            record.resume(&address, &digest),
            Ok(HookResume::Retry {
                invocation_id: record.invocation_id,
            })
        );
    }

    #[test]
    fn completed_hook_resume_reuses_the_committed_decision() {
        let address = hook_address(0);
        let digest = hook_digest('a');
        let mut record = HookInvocationRecord::pending(address.clone(), digest.clone());
        record.state = HookInvocationState::Completed { decision: true };

        assert_eq!(
            record.resume(&address, &digest),
            Ok(HookResume::Reuse {
                invocation_id: record.invocation_id,
                decision: &true,
            })
        );
    }

    #[test]
    fn hook_resume_preserves_terminal_failures() {
        let address = hook_address(0);
        let digest = hook_digest('a');
        let mut record = HookInvocationRecord::<bool>::pending(address.clone(), digest.clone());

        for (state, expected) in [
            (
                HookInvocationState::Failed {
                    failure: HookFailure::InvalidOutput,
                },
                HookFailure::InvalidOutput,
            ),
            (
                HookInvocationState::Failed {
                    failure: HookFailure::HandlerUnavailable,
                },
                HookFailure::HandlerUnavailable,
            ),
            (HookInvocationState::TimedOut, HookFailure::TimedOut),
            (HookInvocationState::Cancelled, HookFailure::Cancelled),
        ] {
            record.state = state;
            assert_eq!(record.resume(&address, &digest), Err(expected));
        }
    }

    #[test]
    fn hook_resume_fails_closed_on_address_version_or_input_mismatch() {
        let address = hook_address(0);
        let digest = hook_digest('a');
        let record = HookInvocationRecord::<bool>::pending(address.clone(), digest.clone());

        let mut wrong_address = address.clone();
        wrong_address.handler_position = 1;
        assert_eq!(
            record.resume(&wrong_address, &digest),
            Err(HookFailure::AddressMismatch)
        );

        let mut wrong_version = address.clone();
        wrong_version.handler_version_id = HookHandlerVersionId::new();
        assert_eq!(
            record.resume(&wrong_version, &digest),
            Err(HookFailure::VersionMismatch)
        );
        assert_eq!(
            record.resume(&address, &hook_digest('b')),
            Err(HookFailure::InputMismatch)
        );
    }

    #[test]
    fn extension_forms_expose_their_minimum_trust_boundaries() {
        let skill = ExtensionForm::Skill.boundary();
        assert!(!skill.may_elevate_permissions);
        assert!(skill.redact_sensitive_payloads);

        let script = ExtensionForm::Script.boundary();
        assert!(script.requires_process_isolation);
        assert!(script.requires_resource_limits);

        let linked = ExtensionForm::LinkedRust.boundary();
        assert!(linked.requires_runtime_compatibility_pin);

        let service = ExtensionForm::HookService.boundary();
        assert!(service.requires_authenticated_transport);
        assert!(service.requires_service_identity_pin);
        assert!(service.requires_invocation_idempotency);
    }
}
