"""
NEXUS Language & Model Schemas
Defines the intermediate representations for intents, action plans, verification,
and model capability metadata across the NEXUS agent runtime.
"""

from typing import Any, Dict, List, Optional
from enum import Enum
from pydantic import BaseModel, Field


class IntentType(str, Enum):
    PROJECT_WORK = "project_work"
    SYSTEM_COMMAND = "system_command"
    MEMORY_RETRIEVAL = "memory_retrieval"
    KNOWLEDGE_QUERY = "knowledge_query"
    CODE_REPAIR = "code_repair"
    CREATIVE_THOUGHT = "creative_thought"
    TASK_ORCHESTRATION = "task_orchestration"
    CLARIFICATION_NEEDED = "clarification_needed"


class RiskLevel(str, Enum):
    LOW = "low"          # Read-only operations, queries, safe lookups
    MEDIUM = "medium"    # Writing to project files, safe git commands
    HIGH = "high"        # Deleting files, running unrestricted shell scripts, git push


class LatencyClass(str, Enum):
    FAST = "fast"        # < 300ms (1-3B personal SLM)
    MEDIUM = "medium"    # 300ms - 2000ms (7-8B local reasoner)
    SLOW = "slow"        # > 2000ms (Deep cloud model or heavy multi-step reasoning)


class PrivacyTier(str, Enum):
    LOCAL = "local"      # Strictly zero external network transmission
    CLOUD = "cloud"      # External API call permitted


class ModelCapabilityScores(BaseModel):
    reasoning: int = Field(default=3, ge=1, le=5, description="1 to 5 capability score")
    coding: int = Field(default=3, ge=1, le=5)
    memory_extraction: int = Field(default=3, ge=1, le=5)
    structured_json: int = Field(default=4, ge=1, le=5)


class ModelCapability(BaseModel):
    """Declared capabilities of a pluggable model brain."""
    context_window: int = 32768
    tool_calling: bool = True
    vision: bool = False
    scores: ModelCapabilityScores = Field(default_factory=ModelCapabilityScores)
    latency_class: LatencyClass = LatencyClass.FAST
    privacy: PrivacyTier = PrivacyTier.LOCAL
    vram_mb: int = 2000


class ModelMetadata(BaseModel):
    name: str
    provider_type: str  # "local_ollama", "local_llamacpp", "cloud_gemini", "mock"
    endpoint: Optional[str] = None
    capabilities: ModelCapability = Field(default_factory=ModelCapability)
    is_active: bool = False


class NexusIntent(BaseModel):
    """The structured representation of a user's prompt."""
    raw_query: str
    intent: IntentType
    goal: str
    target_project: Optional[str] = None
    entities: Dict[str, Any] = Field(default_factory=dict)
    requires_memory: bool = False
    requires_tools: bool = False
    requires_deep_reasoning: bool = False
    risk: RiskLevel = RiskLevel.LOW
    candidate_tools: List[str] = Field(default_factory=list)
    confidence: float = Field(default=1.0, ge=0.0, le=1.0)


class ActionStep(BaseModel):
    """A single atomic action in an action plan."""
    step_id: int
    tool_name: str
    arguments: Dict[str, Any] = Field(default_factory=dict)
    expected_outcome: str
    risk_level: RiskLevel = RiskLevel.LOW


class ActionPlan(BaseModel):
    """An ordered sequence of proposed tool calls."""
    plan_id: str
    goal: str
    steps: List[ActionStep] = Field(default_factory=list)
    estimated_latency: LatencyClass = LatencyClass.FAST
    requires_confirmation: bool = False


class StepObservation(BaseModel):
    """The observation result produced after executing a tool step."""
    step_id: int
    tool_name: str
    status: str  # "success" | "failure"
    output: Any
    error: Optional[str] = None
    duration_ms: float = 0.0


class VerificationResult(BaseModel):
    """Deterministic evaluation of whether an execution succeeded."""
    passed: bool
    reason: str
    failed_step_id: Optional[int] = None
    suggested_repair: Optional[str] = None
    confidence: float = Field(default=1.0, ge=0.0, le=1.0)
