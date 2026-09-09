"""
Context management system for NEXUS
Maintains and updates contextual awareness of user, tasks, and environment
"""

from typing import Any, Dict, List, Optional
from datetime import datetime, timedelta
import asyncio
import logging
from dataclasses import dataclass, field

logger = logging.getLogger(__name__)


@dataclass
class UserProfile:
    """User profile information"""
    user_id: str
    name: Optional[str] = None
    preferences: Dict[str, Any] = field(default_factory=dict)
    expertise_areas: List[str] = field(default_factory=list)
    goals: List[str] = field(default_factory=list)
    communication_style: str = "professional_warm"  # formal, casual, playful, etc.
    detail_tolerance: str = "medium"  # low, medium, high
    proactivity_preference: str = "balanced"  # low, balanced, high
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)


@dataclass
class SessionContext:
    """Current session context"""
    session_id: str
    user_id: str
    start_time: datetime = field(default_factory=datetime.now)
    last_activity: datetime = field(default_factory=datetime.now)
    current_task: Optional[str] = None
    recent_interactions: List[Dict[str, Any]] = field(default_factory=list)
    emotional_state: Optional[str] = None  # inferred from language patterns
    cognitive_load: str = "medium"  # low, medium, high
    energy_level: str = "medium"  # low, medium, high


@dataclass
class ProjectContext:
    """Context for active projects"""
    project_id: str
    name: str
    description: Optional[str] = None
    goals: List[str] = field(default_factory=list)
    deadlines: List[datetime] = field(default_factory=list)
    stakeholders: List[str] = field(default_factory=list)
    progress: float = 0.0  # 0.0 to 1.0
    blockers: List[str] = field(default_factory=list)
    next_actions: List[str] = field(default_factory=list)
    updated_at: datetime = field(default_factory=datetime.now)


class ContextManager:
    """
    Manages contextual awareness for NEXUS, including user profile,
    session context, project context, and environmental factors.
    """

    def __init__(self, user_id: str = "default_user"):
        self.user_id = user_id
        self.user_profile = UserProfile(user_id=user_id)
        self.session_context: Optional[SessionContext] = None
        self.project_contexts: Dict[str, ProjectContext] = {}
        self.context_history: List[Dict[str, Any]] = []
        self.preferences_cache: Dict[str, Any] = {}

    async def initialize_session(self, session_id: str) -> SessionContext:
        """
        Initialize a new session context.

        Args:
            session_id: Unique identifier for the session

        Returns:
            Initialized SessionContext
        """
        self.session_context = SessionContext(
            session_id=session_id,
            user_id=self.user_id
        )
        logger.info(f"Initialized session {session_id} for user {self.user_id}")
        return self.session_context

    async def update_context(
        self,
        query: str,
        response: Dict[str, Any],
        additional_context: Optional[Dict[str, Any]] = None
    ) -> None:
        """
        Update context based on the latest interaction.

        Args:
            query: User's query
            response: NEXUS's response
            additional_context: Optional additional context information
        """
        if not self.session_context:
            await self.initialize_session(f"session_{datetime.now().timestamp()}")

        # Update session activity
        self.session_context.last_activity = datetime.now()

        # Add interaction to recent history
        interaction = {
            "timestamp": datetime.now().isoformat(),
            "query": query,
            "response_summary": response.get("response", "")[:100] + "...",
            "reasoning_mode": response.get("reasoning_mode"),
            "confidence": response.get("confidence")
        }

        self.session_context.recent_interactions.append(interaction)

        # Keep only recent interactions (last 10)
        if len(self.session_context.recent_interactions) > 10:
            self.session_context.recent_interactions = self.session_context.recent_interactions[-10:]

        # Update contextual inferences
        await self._update_contextual_inferences(query, response)

        # Add to context history
        self.context_history.append({
            "timestamp": datetime.now().isoformat(),
            "session_id": self.session_context.session_id,
            "query": query,
            "response": response
        })

        # Keep history manageable
        if len(self.context_history) > 100:
            self.context_history = self.context_history[-100:]

        logger.debug(f"Updated context for session {self.session_context.session_id}")

    async def _update_contextual_inferences(
        self,
        query: str,
        response: Dict[str, Any]
    ) -> None:
        """
        Update inferred contextual information based on interaction.

        Args:
            query: User's query
            response: NEXUS's response
        """
        query_lower = query.lower()

        # Infer emotional state from language
        if any(word in query_lower for word in ["frustrated", "upset", "annoyed", "angry"]):
            self.session_context.emotional_state = "frustrated"
        elif any(word in query_lower for word in ["happy", "excited", "pleased", "great"]):
            self.session_context.emotional_state = "positive"
        elif any(word in query_lower for word in ["tired", "exhausted", "weary"]):
            self.session_context.emotional_state = "tired"
        else:
            self.session_context.emotional_state = "neutral"

        # Infer cognitive load
        if any(word in query_lower for word in ["complex", "complicated", "difficult", "hard"]):
            self.session_context.cognitive_load = "high"
        elif any(word in query_lower for word in ["simple", "easy", "straightforward"]):
            self.session_context.cognitive_load = "low"
        else:
            # Default based on query length and reasoning mode used
            word_count = len(query.split())
            reasoning_mode = response.get("reasoning_mode")
            if word_count > 15 or reasoning_mode == "system_2":
                self.session_context.cognitive_load = "high"
            elif word_count < 8:
                self.session_context.cognitive_load = "low"
            else:
                self.session_context.cognitive_load = "medium"

        # Update current task if indicated
        if any(word in query_lower for word in ["working on", "task", "project", "trying to"]):
            # Extract task mention (simplified)
            self.session_context.current_task = query[:100]  # Truncate for storage

    def get_current_context(self) -> Dict[str, Any]:
        """
        Get the current contextual state.

        Returns:
            Dictionary containing current context information
        """
        if not self.session_context:
            return {"error": "No active session"}

        return {
            "user_profile": {
                "user_id": self.user_profile.user_id,
                "name": self.user_profile.name,
                "expertise_areas": self.user_profile.expertise_areas,
                "goals": self.user_profile.goals,
                "communication_style": self.user_profile.communication_style,
                "detail_tolerance": self.user_profile.detail_tolerance,
                "proactivity_preference": self.user_profile.proactivity_preference
            },
            "session_context": {
                "session_id": self.session_context.session_id,
                "duration_minutes": (datetime.now() - self.session_context.start_time).total_seconds() / 60,
                "current_task": self.session_context.current_task,
                "recent_interactions_count": len(self.session_context.recent_interactions),
                "emotional_state": self.session_context.emotional_state,
                "cognitive_load": self.session_context.cognitive_load,
                "energy_level": self.session_context.energy_level
            },
            "active_projects": [
                {
                    "project_id": pid,
                    "name": proj.name,
                    "progress": proj.progress,
                    "blockers": proj.blockers,
                    "next_actions": proj.next_actions[:3]  # Limit to first 3
                }
                for pid, proj in self.project_contexts.items()
            ],
            "context_history_length": len(self.context_history)
        }

    def update_user_preference(self, key: str, value: Any) -> None:
        """
        Update a user preference.

        Args:
            key: Preference key
            value: Preference value
        """
        self.user_profile.preferences[key] = value
        self.user_profile.updated_at = datetime.now()
        self.preferences_cache[key] = value
        logger.info(f"Updated user preference: {key} = {value}")

    def get_user_preference(self, key: str, default: Any = None) -> Any:
        """
        Get a user preference.

        Args:
            key: Preference key
            default: Default value if key not found

        Returns:
            Preference value or default
        """
        return self.preferences_cache.get(key, self.user_profile.preferences.get(key, default))

    def add_project_context(self, project_context: ProjectContext) -> None:
        """
        Add or update a project context.

        Args:
            project_context: ProjectContext to add or update
        """
        self.project_contexts[project_context.project_id] = project_context
        logger.info(f"Added/updated project context: {project_context.name}")

    def remove_project_context(self, project_id: str) -> bool:
        """
        Remove a project context.

        Args:
            project_id: ID of project to remove

        Returns:
            True if removed, False if not found
        """
        if project_id in self.project_contexts:
            del self.project_contexts[project_id]
            logger.info(f"Removed project context: {project_id}")
            return True
        return False

    async def get_contextual_prompt(self, base_prompt: str) -> str:
        """
        Generate a context-aware prompt enhancement.

        Args:
            base_prompt: Base prompt to enhance

        Returns:
            Enhanced prompt with contextual information
        """
        if not self.session_context:
            return base_prompt

        context_info = []
        context_info.append(f"Current time: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")

        if self.session_context.current_task:
            context_info.append(f"User's current task: {self.session_context.current_task}")

        if self.session_context.emotional_state:
            context_info.append(f"User's emotional state: {self.session_context.emotional_state}")

        if self.session_context.cognitive_load != "medium":
            context_info.append(f"User's cognitive load: {self.session_context.cognitive_load}")

        if self.user_profile.expertise_areas:
            context_info.append(f"User's expertise areas: {', '.join(self.user_profile.expertise_areas[:3])}")

        if self.user_profile.goals:
            context_info.append(f"User's current goals: {', '.join(self.user_profile.goals[:3])}")

        if context_info:
            enhanced_prompt = f"{base_prompt}\n\nContextual Information:\n" + "\n".join(
                f"- {info}" for info in context_info
            )
            return enhanced_prompt

        return base_prompt


# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # Initialize context manager
        ctx_manager = ContextManager(user_id="test_user_123")

        # Initialize a session
        session = await ctx_manager.initialize_session("session_001")
        print(f"Initialized session: {session.session_id}")

        # Simulate an interaction
        query = "I'm feeling frustrated with my current project deadline. Can you help me analyze the situation?"
        response = {
            "reasoning_mode": "system_2",
            "response": "I understand you're feeling frustrated...",
            "confidence": 0.8
        }

        await ctx_manager.update_context(query, response)

        # Get current context
        current_ctx = ctx_manager.get_current_context()
        print("\nCurrent Context:")
        print(f"Emotional State: {current_ctx['session_context']['emotional_state']}")
        print(f"Cognitive Load: {current_ctx['session_context']['cognitive_load']}")

        # Update a preference
        ctx_manager.update_user_preference("communication_style", "casual")
        print(f"\nUpdated preference: {ctx_manager.get_user_preference('communication_style')}")

    asyncio.run(main())