"""
Specialized operational modes for NEXUS
Implements Deep Focus, Learning Companion, Creative Collaboration, and Strategic Planning modes
"""

from typing import Any, Dict, List, Optional
from datetime import datetime, timedelta
import asyncio
import logging
from dataclasses import dataclass, field
from enum import Enum

from ..core.memory import MemoryEntry, MemoryType, MemorySystem
from ..core.reasoning import ReasoningMode

logger = logging.getLogger(__name__)


class ModeType(Enum):
    """Types of specialized modes"""
    DEEP_FOCUS = "deep_focus"
    LEARNING_COMPANION = "learning_companion"
    CREATIVE_COLLABORATION = "creative_collaboration"
    STRATEGIC_PLANNING = "strategic_planning"


@dataclass
class ModeContext:
    """Context for a specialized mode"""
    mode_type: ModeType
    activated_at: datetime = field(default_factory=datetime.now)
    user_goal: Optional[str] = None
    settings: Dict[str, Any] = field(default_factory=dict)
    session_data: Dict[str, Any] = field(default_factory=dict)
    is_active: bool = True


class SpecializedModes:
    """
    Implements specialized operational modes for different user contexts and needs.
    """

    def __init__(
        self,
        reasoning_engine: Optional['ReasoningEngine'] = None,
        context_manager: Optional['ContextManager'] = None,
        memory_system: Optional['MemorySystem'] = None,
        task_orchestrator: Optional['TaskOrchestrator'] = None,
        information_synthesizer: Optional['InformationSynthesizer'] = None,
        creative_collaborator: Optional['CreativeCollaborator'] = None,
        decision_analyzer: Optional['DecisionAnalyzer'] = None
    ):
        self.reasoning_engine = reasoning_engine
        self.context_manager = context_manager
        self.memory_system = memory_system
        self.task_orchestrator = task_orchestrator
        self.information_synthesizer = information_synthesizer
        self.creative_collaborator = creative_collaborator
        self.decision_analyzer = decision_analyzer

        self.active_modes: Dict[ModeType, ModeContext] = {}
        self.mode_history: List[Dict[str, Any]] = []

    async def activate_mode(
        self,
        mode_type: ModeType,
        user_goal: Optional[str] = None,
        settings: Optional[Dict[str, Any]] = None
    ) -> ModeContext:
        """
        Activate a specialized mode.

        Args:
            mode_type: Type of mode to activate
            user_goal: Optional goal for this mode session
            settings: Optional mode-specific settings

        Returns:
            ModeContext for the activated mode
        """
        # Deactivate conflicting modes if any
        await self._handle_mode_conflicts(mode_type)

        # Create mode context
        mode_context = ModeContext(
            mode_type=mode_type,
            user_goal=user_goal,
            settings=settings or {},
            session_data={}
        )

        # Store active mode
        self.active_modes[mode_type] = mode_context

        # Log to history
        self.mode_history.append({
            "timestamp": datetime.now().isoformat(),
            "mode_type": mode_type.value,
            "action": "activated",
            "user_goal": user_goal
        })

        # Initialize mode-specific setup
        await self._initialize_mode(mode_context)

        logger.info(f"Activated {mode_type.value} mode")
        return mode_context

    async def deactivate_mode(self, mode_type: ModeType) -> bool:
        """
        Deactivate a specialized mode.

        Args:
            mode_type: Type of mode to deactivate

        Returns:
            True if deactivated, False if not active
        """
        if mode_type not in self.active_modes:
            return False

        mode_context = self.active_modes[mode_type]
        mode_context.is_active = False

        # Log to history
        self.mode_history.append({
            "timestamp": datetime.now().isoformat(),
            "mode_type": mode_type.value,
            "action": "deactivated",
            "user_goal": mode_context.user_goal
        })

        # Remove from active modes
        del self.active_modes[mode_type]

        logger.info(f"Deactivated {mode_type.value} mode")
        return True

    async def _handle_mode_conflicts(self, mode_type: ModeType) -> None:
        """
        Handle conflicts between modes (e.g., Deep Focus and Learning Companion might conflict).

        Args:
            mode_type: The mode being activated
        """
        # Define conflicting mode pairs
        conflicts = {
            ModeType.DEEP_FOCUS: [ModeType.LEARNING_COMPANION, ModeType.CREATIVE_COLLABORATION],
            ModeType.LEARNING_COMPANION: [ModeType.DEEP_FOCUS],
            ModeType.CREATIVE_COLLABORATION: [ModeType.DEEP_FOCUS],
            ModeType.STRATEGIC_PLANNING: []  # Strategic planning can coexist with others
        }

        # Deactivate conflicting modes
        if mode_type in conflicts:
            for conflicting_mode in conflicts[mode_type]:
                if conflicting_mode in self.active_modes:
                    await self.deactivate_mode(conflicting_mode)

    async def _initialize_mode(self, mode_context: ModeContext) -> None:
        """
        Initialize mode-specific setup.

        Args:
            mode_context: Context for the mode to initialize
        """
        mode_type = mode_context.mode_type

        if mode_type == ModeType.DEEP_FOCUS:
            await self._initialize_deep_focus(mode_context)
        elif mode_type == ModeType.LEARNING_COMPANION:
            await self._initialize_learning_companion(mode_context)
        elif mode_type == ModeType.CREATIVE_COLLABORATION:
            await self._initialize_creative_collaboration(mode_context)
        elif mode_type == ModeType.STRATEGIC_PLANNING:
            await self._initialize_strategic_planning(mode_context)

    async def _initialize_deep_focus(self, mode_context: ModeContext) -> None:
        """Initialize Deep Focus mode"""
        mode_context.session_data.update({
            "focus_start": datetime.now(),
            "distractions_blocked": 0,
            "deep_work_sessions": 0,
            "break_suggested": False
        })
        logger.debug("Deep Focus mode initialized")

    async def _initialize_learning_companion(self, mode_context: ModeContext) -> None:
        """Initialize Learning Companion mode"""
        mode_context.session_data.update({
            "learning_goal": mode_context.user_goal,
            "current_topic": None,
            "resources_explored": 0,
            "practice_exercises_completed": 0,
            "mastery_level": 0.0  # 0.0 to 1.0
        })
        logger.debug("Learning Companion mode initialized")

    async def _initialize_creative_collaboration(self, mode_context: ModeContext) -> None:
        """Initialize Creative Collaboration mode"""
        mode_context.session_data.update({
            "brainstorm_sessions": 0,
            "ideas_generated": 0,
            "ideas_refined": 0,
            "current_technique": None
        })
        logger.debug("Creative Collaboration mode initialized")

    async def _initialize_strategic_planning(self, mode_context: ModeContext) -> None:
        """Initialize Strategic Planning mode"""
        mode_context.session_data.update({
            "planning_horizon": mode_context.settings.get("horizon", "quarterly"),  # monthly, quarterly, yearly
            "goals_identified": 0,
            "options_explored": 0,
            "plan_developed": False
        })
        logger.debug("Strategic Planning mode initialized")

    async def process_in_mode(
        self,
        mode_type: ModeType,
        user_input: str,
        context: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """
        Process user input within a specific mode.

        Args:
            mode_type: The mode to process in
            user_input: User's input/query
            context: Optional additional context

        Returns:
            Dictionary containing response and mode-specific data
        """
        if mode_type not in self.active_modes:
            return {
                "error": f"Mode {mode_type.value} is not active",
                "suggestion": f"Activate {mode_type.value} mode first"
            }

        mode_context = self.active_modes[mode_type]
        mode_context.session_data["last_interaction"] = datetime.now().isoformat()

        # Route to mode-specific processor
        if mode_type == ModeType.DEEP_FOCUS:
            return await self._process_deep_focus(user_input, mode_context, context)
        elif mode_type == ModeType.LEARNING_COMPANION:
            return await self._process_learning_companion(user_input, mode_context, context)
        elif mode_type == ModeType.CREATIVE_COLLABORATION:
            return await self._process_creative_collaboration(user_input, mode_context, context)
        elif mode_type == ModeType.STRATEGIC_PLANNING:
            return await self._process_strategic_planning(user_input, mode_context, context)
        else:
            return {"error": f"Unknown mode type: {mode_type}"}

    async def _process_deep_focus(
        self,
        user_input: str,
        mode_context: ModeContext,
        context: Optional[Dict[str, Any]]
    ) -> Dict[str, Any]:
        """Process input in Deep Focus mode"""
        # Update session data
        if "current_task" not in mode_context.session_data:
            mode_context.session_data["current_task"] = user_input[:100]  # Store first 100 chars as task
            mode_context.session_data["focus_start"] = datetime.now()

        # Check if user wants to break focus
        if any(word in user_input.lower() for word in ["break", "rest", "stop", "pause"]):
            # Suggest ending the focus session
            focus_duration = (datetime.now() - mode_context.session_data["focus_start"]).total_seconds() / 60
            mode_context.session_data["break_suggested"] = True
            return {
                "response": f"You've been in deep focus for {focus_duration:.1f} minutes. Would you like to take a break?",
                "mode": "deep_focus",
                "suggested_break": True,
                "focus_duration_minutes": round(focus_duration, 1)
            }

        # For deep focus, we want minimal, direct responses
        # Use System 1 reasoning for quick answers
        if self.reasoning_engine:
            reasoning_result = await self.reasoning_engine.process_query(
                user_input,
                context=context,
                mode=ReasoningMode.SYSTEM_1  # Force rapid response
            )
            response = reasoning_result["response"]
        else:
            response = f"I understand you're focused on: {user_input[:50]}..."

        # Update distraction count if input seems off-task
        current_task = mode_context.session_data.get("current_task", "")
        if current_task and user_input.lower() not in current_task.lower() and len(user_input) > 10:
            mode_context.session_data["distractions_blocked"] = mode_context.session_data.get("distractions_blocked", 0) + 1

        return {
            "response": response,
            "mode": "deep_focus",
            "focus_protected": True,
            "distractions_blocked": mode_context.session_data.get("distractions_blocked", 0)
        }

    async def _process_learning_companion(
        self,
        user_input: str,
        mode_context: ModeContext,
        context: Optional[Dict[str, Any]]
    ) -> Dict[str, Any]:
        """Process input in Learning Companion mode"""
        # If we don't have a learning goal yet, set it from user input
        if not mode_context.session_data.get("learning_goal") and mode_context.user_goal:
            mode_context.session_data["learning_goal"] = mode_context.user_goal

        # Determine if user wants to learn something specific
        learning_indicators = ["learn", "understand", "explain", "teach me", "how does", "what is"]
        is_learning_request = any(indicator in user_input.lower() for indicator in learning_indicators)

        if is_learning_request:
            # Use the information synthesizer to research and synthesize
            if self.information_synthesizer:
                # Create a research query
                from .information_synthesis import ResearchQuery, SummaryStyle
                query = ResearchQuery(
                    id=f"learn_{datetime.now().timestamp()}",
                    question=user_input,
                    context=f"Learning goal: {mode_context.session_data.get('learning_goal')}",
                    depth="medium",
                    sources_required=3,
                    summary_style=SummaryStyle.EXECUTIVE
                )
                synthesis_result = await self.information_synthesizer.research_and_synthesize(query)

                # Store the learning interaction
                if self.memory_system:
                    memory_entry = MemoryEntry(
                        id=f"learn_{len(self.memory_system._cache)}",
                        memory_type=MemoryType.SEMANTIC,
                        content={
                            "learning_goal": mode_context.session_data.get('learning_goal'),
                            "query": user_input,
                            "synthesis": synthesis_result.to_dict()
                        },
                        importance_score=0.7,
                        tags=["learning", mode_context.session_data.get('learning_goal', '').replace(' ', '_')],
                        expires_at=None
                    )
                    await self.memory_system.store_memory(memory_entry)

                # Update session data
                mode_context.session_data["resources_explored"] = mode_context.session_data.get("resources_explored", 0) + 1

                return {
                    "response": synthesis_result.summary,
                    "mode": "learning_companion",
                    "key_insights": synthesis_result.key_insights,
                    "sources_used": len(synthesis_result.sources),
                    "confidence": synthesis_result.confidence_score,
                    "suggestion": "Would you like me to create a practice exercise or explore any of these points further?"
                }
            else:
                # Fallback if synthesizer not available
                response = f"I'd help you learn about: {user_input}"
                if self.reasoning_engine:
                    reasoning_result = await self.reasoning_engine.process_query(
                        user_input,
                        context=context,
                        mode=ReasoningMode.SYSTEM_2
                    )
                    response = reasoning_result["response"]
                return {
                    "response": response,
                    "mode": "learning_companion",
                    "note": "Learning companion mode active"
                }
        else:
            # General conversation in learning mode
            if self.reasoning_engine:
                reasoning_result = await self.reasoning_engine.process_query(
                    user_input,
                    context=context,
                    mode=ReasoningMode.SYSTEM_1
                )
                response = reasoning_result["response"]
            else:
                response = f"I'm here to help you learn. What would you like to explore?"

            return {
                "response": response,
                "mode": "learning_companion",
                "learning_goal": mode_context.session_data.get("learning_goal")
            }

    async def _process_creative_collaboration(
        self,
        user_input: str,
        mode_context: ModeContext,
        context: Optional[Dict[str, Any]]
    ) -> Dict[str, Any]:
        """Process input in Creative Collaboration mode"""
        # Check if user wants to brainstorm or generate ideas
        creativity_indicators = ["brainstorm", "ideas", "creative", "innovate", "think of", "suggest", "what if"]
        is_creative_request = any(indicator in user_input.lower() for indicator in creativity_indicators)

        if is_creative_request and self.creative_collaborator:
            # Conduct a brainstorming session
            brainstorm_result = await self.creative_collaborator.brainstorm(
                topic=user_input,
                goal="Generate creative ideas and solutions",
                technique=None,  # Let the collaborator choose
                num_ideas=5
            )

            # Update session data
            mode_context.session_data["brainstorm_sessions"] = mode_context.session_data.get("brainstorm_sessions", 0) + 1
            mode_context.session_data["ideas_generated"] = mode_context.session_data.get("ideas_generated", 0) + brainstorm_result["ideas_generated"]

            return {
                "response": f"I've generated {brainstorm_result['ideas_generated']} creative ideas for you.",
                "mode": "creative_collaboration",
                "brainstorm_session_id": brainstorm_result["session_id"],
                "ideas": brainstorm_result["ideas"][:3],  # Return top 3 ideas
                "suggestion": "Would you like to explore any of these ideas further, or try a different creativity technique?"
            }
        else:
            # General creative conversation
            if self.reasoning_engine:
                reasoning_result = await self.reasoning_engine.process_query(
                    user_input,
                    context=context,
                    mode=ReasoningMode.SYSTEM_2  # Use deeper reasoning for creative discussions
                )
                response = reasoning_result["response"]
            else:
                response = f"Let's think creatively about: {user_input}"

            return {
                "response": response,
                "mode": "creative_collaboration",
                "note": "Creative collaboration mode active - feel free to ask for brainstorming or idea generation"
            }

    async def _process_strategic_planning(
        self,
        user_input: str,
        mode_context: ModeContext,
        context: Optional[Dict[str, Any]]
    ) -> Dict[str, Any]:
        """Process input in Strategic Planning mode"""
        # Check if user wants to set goals, analyze options, or create a plan
        planning_indicators = ["goal", "objective", "plan", "strategy", "analyze", "compare", "decide", "option"]
        is_planning_request = any(indicator in user_input.lower() for indicator in planning_indicators)

        if is_planning_request:
            # Use decision analyzer or task orchestrator for planning
            if "decide" in user_input.lower() or "choose" in user_input.lower() or "option" in user_input.lower():
                # This is a decision - use decision analyzer
                if self.decision_analyzer:
                    # We'd need to set up criteria and options - for now, provide a framework
                    return {
                        "response": "I can help you structure this decision. To proceed, I'll need:\n1. What are the options you're considering?\n2. What criteria are important to you?\n3. How would you weight each criterion?",
                        "mode": "strategic_planning",
                        "planning_type": "decision_analysis",
                        "next_steps": ["Define options", "Establish criteria", "Weight criteria"]
                    }
                else:
                    # Fallback to reasoning
                    if self.reasoning_engine:
                        reasoning_result = await self.reasoning_engine.process_query(
                            user_input,
                            context=context,
                            mode=ReasoningMode.SYSTEM_2
                        )
                        response = reasoning_result["response"]
                    else:
                        response = f"Let's analyze this strategically: {user_input}"
                    return {
                        "response": response,
                        "mode": "strategic_planning"
                    }
            else:
                # General planning - use task orchestrator to break down goals
                if self.task_orchestrator:
                    # Decompose the user's input as a goal
                    goal_result = await self.task_orchestrator.decompose_goal(
                        goal=user_input,
                        context=context,
                        timeframe=mode_context.session_data.get("planning_horizon"),
                        resources=None
                    )

                    # Update session data
                    mode_context.session_data["goals_identified"] = mode_context.session_data.get("goals_identified", 0) + 1
                    tasks_count = goal_result.get("tasks_created", 0)
                    if isinstance(tasks_count, list):
                        tasks_count = len(tasks_count)
                    mode_context.session_data["options_explored"] = mode_context.session_data.get("options_explored", 0) + tasks_count

                    return {
                        "response": f"I've broken down your goal into a project with {goal_result['tasks_created']} initial tasks.",
                        "mode": "strategic_planning",
                        "project_id": goal_result["project_id"],
                        "estimated_effort_hours": goal_result["estimated_total_effort"],
                        "next_steps": goal_result["next_steps"]
                    }
                else:
                    # Fallback to reasoning
                    if self.reasoning_engine:
                        reasoning_result = await self.reasoning_engine.process_query(
                            user_input,
                            context=context,
                            mode=ReasoningMode.SYSTEM_2
                        )
                        response = reasoning_result["response"]
                    else:
                        response = f"Let's think about this strategically: {user_input}"
                    return {
                        "response": response,
                        "mode": "strategic_planning"
                    }
        else:
            # General strategic conversation
            if self.reasoning_engine:
                reasoning_result = await self.reasoning_engine.process_query(
                    user_input,
                    context=context,
                    mode=ReasoningMode.SYSTEM_1
                )
                response = reasoning_result["response"]
            else:
                response = f"I'm in strategic planning mode. How can I help you think long-term?"

            return {
                "response": response,
                "mode": "strategic_planning",
                "planning_horizon": mode_context.session_data.get("planning_horizon")
            }

    def get_active_modes(self) -> List[Dict[str, Any]]:
        """Get information about currently active modes"""
        active = []
        for mode_type, context in self.active_modes.items():
            active.append({
                "mode_type": mode_type.value,
                "activated_at": context.activated_at.isoformat(),
                "user_goal": context.user_goal,
                "is_active": context.is_active,
                "session_duration_minutes": (datetime.now() - context.activated_at).total_seconds() / 60
            })
        return active

    def get_mode_history(self, limit: int = 10) -> List[Dict[str, Any]]:
        """Get recent mode history"""
        return self.mode_history[-limit:] if self.mode_history else []

# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # This is a simplified example - in reality, these would be properly initialized
        print("Specialized Modes module loaded")
        print("Available modes:", [mode.value for mode in ModeType])

    asyncio.run(main())