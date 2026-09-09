"""
NEXUS Core Adapter for Web Interface
This adapter wraps the existing NEXUS components to provide a simple API for the web interface
"""

import asyncio
from typing import Optional, Dict, List

# Import NEXUS components
import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from nexus.core.reasoning import ReasoningEngine
from nexus.core.context import ContextManager
from nexus.core.memory import MemorySystem
from nexus.modules.task_orchestration import TaskOrchestrator
from nexus.modules.information_synthesis import InformationSynthesizer
from nexus.modules.creative_collaboration import CreativeCollaborator
from nexus.modules.decision_support import DecisionAnalyzer
from nexus.modules.specialized_modes import SpecializedModes, ModeType
from nexus.modules.skill_learning import SkillLearner, SkillType, SkillLevel


class NexusCore:
    """
    Core NEXUS functionality adapted for web interface
    """

    def __init__(self):
        # Initialize core components (similar to NexusConsole)
        self.memory_system = MemorySystem("nexus_web_memory.db")
        self.context_manager = ContextManager()
        self.reasoning_engine = ReasoningEngine(self.context_manager)
        self.task_orchestrator = TaskOrchestrator(self.memory_system)
        self.information_synthesizer = InformationSynthesizer(self.memory_system)
        self.creative_collaborator = CreativeCollaborator(self.memory_system)
        self.decision_analyzer = DecisionAnalyzer(self.memory_system)
        self.specialized_modes = SpecializedModes(
            reasoning_engine=self.reasoning_engine,
            context_manager=self.context_manager,
            memory_system=self.memory_system,
            task_orchestrator=self.task_orchestrator,
            information_synthesizer=self.information_synthesizer,
            creative_collaborator=self.creative_collaborator,
            decision_analyzer=self.decision_analyzer
        )
        self.skill_learner = SkillLearner(
            memory_system=self.memory_system,
            rag_path=r"D:\Desktop\RAG"  # Using the RAG path mentioned by the user
        )

        # Initialize context
        self.session_id = f"web_{asyncio.get_event_loop().time() if asyncio.get_event_loop().is_running() else 0}"

    async def process_query(self, user_input: str, mode: Optional[str] = None) -> Dict[str, str]:
        """
        Process a user query and return a response
        """
        try:
            # Update context
            await self.context_manager.update_context(
                user_input,
                {"placeholder": "processing"},
                {}
            )

            # Handle mode activation if specified
            if mode:
                try:
                    mode_type = ModeType(mode.lower())
                    await self.specialized_modes.activate_mode(mode_type)
                except ValueError:
                    pass  # Invalid mode, ignore

            # Determine which mode(s) are active and route accordingly
            active_modes = self.specialized_modes.get_active_modes()
            if active_modes:
                # If multiple modes are active, prioritize the most recently activated
                mode_type_str = active_modes[0]["mode_type"]
                mode_type = ModeType(mode_type_str)
                result = await self.specialized_modes.process_in_mode(
                    mode_type, user_input
                )
            else:
                # No active modes - use general reasoning
                reasoning_result = await self.reasoning_engine.process_query(
                    user_input,
                    context=self.context_manager.get_current_context()
                )
                result = reasoning_result

            # Update context with the interaction
            await self.context_manager.update_context(
                user_input,
                result
            )

            return result

        except Exception as e:
            return {
                "response": f"I apologize, but I encountered an error: {str(e)}",
                "error": str(e)
            }

    async def get_skills(self) -> List[Dict]:
        """
        Get list of learned skills
        """
        try:
            skills = await self.skill_learner.list_skills()
            return [
                {
                    "id": skill.id,
                    "name": skill.name,
                    "description": skill.description,
                    "skill_type": skill.skill_type.value,
                    "level": skill.level.value,
                    "proficiency_score": skill.proficiency_score,
                    "learned_at": skill.learned_at.isoformat() if skill.learned_at else None,
                    "resources": skill.resources
                }
                for skill in skills
            ]
        except Exception as e:
            print(f"Error getting skills: {e}")
            return []

    async def get_status(self) -> Dict:
        """
        Get current system status
        """
        try:
            context = self.context_manager.get_current_context()
            active_modes = self.specialized_modes.get_active_modes()

            return {
                "session_id": self.session_id,
                "active_modes": [mode["mode_type"] for mode in active_modes],
                "context": context,
                "memory_stats": self.memory_system.get_memory_stats()
            }
        except Exception as e:
            return {
                "error": str(e)
            }

    async def activate_mode(self, mode_name: str) -> str:
        """
        Activate a specialized mode
        """
        try:
            mode_type = ModeType(mode_name.lower())
            mode_context = await self.specialized_modes.activate_mode(mode_type)
            return f"Activated {mode_type.value} mode."
        except ValueError as e:
            return f"Unknown mode: {mode_name}"
        except Exception as e:
            return f"Error activating mode: {str(e)}"

    async def deactivate_mode(self, mode_name: str) -> str:
        """
        Deactivate a specialized mode
        """
        try:
            mode_type = ModeType(mode_name.lower())
            success = await self.specialized_modes.deactivate_mode(mode_type)
            if success:
                return f"Deactivated {mode_type.value} mode."
            else:
                return f"Mode {mode_type.value} was not active."
        except ValueError as e:
            return f"Unknown mode: {mode_name}"
        except Exception as e:
            return f"Error deactivating mode: {str(e)}"

    async def learn_skill(self, name: str, description: str, skill_type: str = "coding") -> Dict:
        """
        Learn a new skill
        """
        try:
            stype = SkillType(skill_type.lower()) if hasattr(SkillType, skill_type.upper()) else SkillType.CODING
            skill = await self.skill_learner.learn_skill(
                name=name,
                description=description,
                skill_type=stype,
                level=SkillLevel.BEGINNER
            )
            return skill.to_dict()
        except Exception as e:
            return {"error": str(e)}

    async def decompose_goal(self, goal: str, timeframe: str = "1 week") -> Dict:
        """
        Decompose a goal into tasks
        """
        try:
            res = await self.task_orchestrator.decompose_goal(
                goal=goal,
                timeframe=timeframe
            )
            return res
        except Exception as e:
            return {"error": str(e)}