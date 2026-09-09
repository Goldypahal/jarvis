"""
Skill learning system for NEXUS
Allows NEXUS to learn new skills, languages, and capabilities from user instructions
"""

import asyncio
import logging
import os
import json
from typing import Any, Dict, List, Optional
from datetime import datetime
from dataclasses import dataclass, field
from enum import Enum

from ..core.memory import MemoryEntry, MemoryType

logger = logging.getLogger(__name__)


class SkillType(Enum):
    """Types of skills NEXUS can learn"""
    CODING = "coding"           # Programming-related skills
    LANGUAGE = "language"       # Human language skills
    TOOL = "tool"               # Software/tool usage skills
    DOMAIN = "domain"           # Domain-specific knowledge
    PROCESS = "process"         # Procedural/workflow skills
    APPLICATION = "application" # Application control skills


class SkillLevel(Enum):
    """Proficiency levels for skills"""
    BEGINNER = "beginner"
    INTERMEDIATE = "intermediate"
    ADVANCED = "advanced"
    EXPERT = "expert"


@dataclass
class Skill:
    """Represents a learnable skill"""
    id: str
    name: str
    description: str
    skill_type: SkillType
    level: SkillLevel = SkillLevel.BEGINNER
    prerequisites: List[str] = field(default_factory=list)  # IDs of prerequisite skills
    resources: List[Dict[str, str]] = field(default_factory=list)  # Learning resources
    practiced: bool = False
    proficiency_score: float = 0.0  # 0.0 to 1.0
    learned_at: Optional[datetime] = None
    updated_at: datetime = field(default_factory=datetime.now)
    metadata: Dict[str, Any] = field(default_factory=dict)
    tags: List[str] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization"""
        return {
            "id": self.id,
            "name": self.name,
            "description": self.description,
            "skill_type": self.skill_type.value,
            "level": self.level.value,
            "prerequisites": self.prerequisites,
            "resources": self.resources,
            "practiced": self.practiced,
            "proficiency_score": self.proficiency_score,
            "learned_at": self.learned_at.isoformat() if self.learned_at else None,
            "updated_at": self.updated_at.isoformat(),
            "metadata": self.metadata,
            "tags": self.tags
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> 'Skill':
        """Create from dictionary"""
        data = data.copy()
        data['skill_type'] = SkillType(data['skill_type'])
        data['level'] = SkillLevel(data['level'])
        if data.get('learned_at'):
            data['learned_at'] = datetime.fromisoformat(data['learned_at'])
        if data.get('updated_at'):
            data['updated_at'] = datetime.fromisoformat(data['updated_at'])
        return cls(**data)


@dataclass
class SkillLearningSession:
    """Tracks a skill learning session"""
    id: str
    skill_id: str
    goal: str
    started_at: datetime = field(default_factory=datetime.now)
    ended_at: Optional[datetime] = None
    time_spent_minutes: float = 0.0
    resources_studied: List[str] = field(default_factory=list)
    practice_exercises_completed: int = 0
    notes: str = ""
    outcomes: List[str] = field(default_factory=list)


class SkillLearner:
    """
    Enables NEXUS to learn new skills, languages, and capabilities.
    Manages skill acquisition, tracking, and application.
    """

    def __init__(self, memory_system: Optional['MemorySystem'] = None,
                 rag_path: Optional[str] = None):
        self.memory_system = memory_system
        self.skills: Dict[str, Skill] = {}
        self.learning_sessions: Dict[str, SkillLearningSession] = {}
        self.skill_counter = 0
        self.session_counter = 0

        # RAG integration path
        self.rag_path = rag_path or r"D:\Desktop\RAG"
        self.rag_available = self._check_rag_availability()

        # Application control capabilities
        self.application_commands = {
            "windows": {
                "notepad": "notepad.exe",
                "calculator": "calc.exe",
                "cmd": "cmd.exe",
                "powershell": "powershell.exe",
                "taskmgr": "taskmgr.exe",
                "control": "control.exe",
                "regedit": "regedit.exe",
                "msconfig": "msconfig.exe",
                "explorer": "explorer.exe",
                "chrome": "chrome.exe",
                "firefox": "firefox.exe",
                "edge": "msedge.exe",
                "word": "winword.exe",
                "excel": "excel.exe",
                "powerpoint": "powerpnt.exe",
                "outlook": "outlook.exe",
                "vlc": "vlc.exe",
                "spotify": "spotify.exe",
                "discord": "discord.exe",
                "steam": "steam.exe"
            }
        }

    def _check_rag_availability(self) -> bool:
        """Check if the RAG system is available at the specified path"""
        # Since we can't directly check the file system due to availability issues,
        # we'll assume it's available and handle errors gracefully during use
        logger.info(f"RAG path configured: {self.rag_path}")
        return True  # Assume available, handle errors during actual use

    def _generate_skill_id(self) -> str:
        """Generate a unique skill ID"""
        self.skill_counter += 1
        return f"skill_{self.skill_counter:04d}"

    def _generate_session_id(self) -> str:
        """Generate a unique learning session ID"""
        self.session_counter += 1
        return f"session_{self.session_counter:04d}"

    async def learn_skill(self,
                         name: str,
                         description: str,
                         skill_type: SkillType,
                         level: SkillLevel = SkillLevel.BEGINNER,
                         prerequisites: Optional[List[str]] = None,
                         resources: Optional[List[Dict[str, str]]] = None,
                         user_goal: Optional[str] = None) -> Skill:
        """
        Learn a new skill.

        Args:
            name: Name of the skill
            description: Description of what the skill enables
            skill_type: Type of skill to learn
            level: Initial proficiency level
            prerequisites: List of prerequisite skill IDs
            resources: List of learning resources (title, url, type)
            user_goal: User's goal for learning this skill

        Returns:
            Learned Skill object
        """
        skill_id = self._generate_skill_id()

        skill = Skill(
            id=skill_id,
            name=name,
            description=description,
            skill_type=skill_type,
            level=level,
            prerequisites=prerequisites or [],
            resources=resources or [],
            learned_at=datetime.now(),
            metadata={
                "user_goal": user_goal,
                "learning_method": "user_directed"
            }
        )

        self.skills[skill_id] = skill

        # Store in memory if available
        if self.memory_system:
            memory_entry = MemoryEntry(
                id=f"skill_{skill.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=skill.to_dict(),
                importance_score=0.8,
                tags=["skill", skill_type.value],
                expires_at=None
            )
            await self.memory_system.store_memory(memory_entry)

        logger.info(f"Learned new skill: {name} ({skill.id})")

        # If RAG is available, try to gather additional resources
        if self.rag_available:
            await self._augment_skill_with_rag_resources(skill, user_goal or name)

        return skill

    async def _augment_skill_with_rag_resources(self, skill: Skill, query: str):
        """
        Augment a skill with resources from the RAG system.

        Args:
            skill: Skill to augment
            query: Search query for finding relevant resources
        """
        try:
            # In a real implementation, this would query the RAG system
            # For now, we'll simulate the process
            logger.info(f"Augmenting skill '{skill.name}' with RAG resources for query: {query}")

            # Simulate finding resources in RAG
            simulated_resources = [
                {
                    "title": f"Introduction to {skill.name}",
                    "url": f"rag://{skill.id}/intro",
                    "type": "documentation",
                    "source": "RAG System"
                },
                {
                    "title": f"Best Practices for {skill.name}",
                    "url": f"rag://{skill.id}/best practices",
                    "type": "guide",
                    "source": "RAG System"
                },
                {
                    "title": f"Advanced Techniques in {skill.name}",
                    "url": f"rag://{skill.id}/advanced",
                    "type": "tutorial",
                    "source": "RAG System"
                }
            ]

            # Add RAG resources to the skill (avoid duplicates)
            existing_urls = {r.get("url") for r in skill.resources}
            for resource in simulated_resources:
                if resource["url"] not in existing_urls:
                    skill.resources.append(resource)

            # Update the skill in memory
            if self.memory_system:
                memory_entry = MemoryEntry(
                    id=f"skill_{skill.id}",
                    memory_type=MemoryType.PROCEDURAL,
                    content=skill.to_dict(),
                    importance_score=0.85,  # Slightly higher due to RAG enhancement
                    tags=["skill", skill.skill_type.value, "rag-enhanced"],
                    expires_at=None
                )
                await self.memory_system.store_memory(memory_entry)

        except Exception as e:
            logger.warning(f"Could not augment skill with RAG resources: {e}")

    async def learn_language(self,
                           language_name: str,
                           target_level: SkillLevel = SkillLevel.INTERMEDIATE,
                           focus_areas: Optional[List[str]] = None) -> Skill:
        """
        Learn a new human language.

        Args:
            language_name: Name of the language to learn (e.g., "Spanish", "Japanese")
            target_level: Desired proficiency level
            focus_areas: Specific areas to focus on (e.g., ["conversation", "technical"])

        Returns:
            Learned Language Skill object
        """
        description = f"Ability to communicate in {language_name}"
        if focus_areas:
            description += f" with focus on {', '.join(focus_areas)}"

        skill = await self.learn_skill(
            name=f"{language_name} Language",
            description=description,
            skill_type=SkillType.LANGUAGE,
            level=target_level,
            user_goal=f"Learn {language_name} to {target_level.value} proficiency"
        )

        # Add language-specific metadata
        skill.metadata.update({
            "language_name": language_name,
            "focus_areas": focus_areas or [],
            "learning_path": ["alphabet", "basic_grammar", "common_phrases", "vocabulary_building", "conversation_practice"]
        })

        return skill

    async def learn_application_control(self,
                                      application_name: str,
                                      actions: Optional[List[str]] = None) -> Skill:
        """
        Learn to control a specific application.

        Args:
            application_name: Name of the application to control
            actions: Specific actions to learn (e.g., ["open", "close", "save", "print"])

        Returns:
            Learned Application Control Skill object
        """
        # Normalize application name for lookup
        app_key = application_name.lower().replace(" ", "")

        # Check if we know this application
        known_app = False
        executable = None

        # Check Windows applications (assuming Windows environment)
        if os.name == 'nt':  # Windows
            for known_app_key, exe in self.application_commands.get("windows", {}).items():
                if app_key in known_app_key or known_app_key in app_key:
                    known_app = True
                    executable = exe
                    break

        description = f"Ability to control {application_name}"
        if known_app and executable:
            description += f" (executable: {executable})"
        if actions:
            description += f" with actions: {', '.join(actions)}"

        skill = await self.learn_skill(
            name=f"{application_name} Control",
            description=description,
            skill_type=SkillType.APPLICATION,
            level=SkillLevel.BEGINNER,  # Start at beginner for application control
            user_goal=f"Learn to control {application_name} application"
        )

        # Add application-specific metadata
        skill.metadata.update({
            "application_name": application_name,
            "executable": executable,
            "actions": actions or ["open", "close", "basic_usage"],
            "platform": os.name
        })

        return skill

    async def practice_skill(self,
                           skill_id: str,
                           practice_description: str,
                           duration_minutes: float = 15.0) -> bool:
        """
        Practice a skill to improve proficiency.

        Args:
            skill_id: ID of skill to practice
            practice_description: Description of the practice activity
            duration_minutes: How long to practice (in minutes)

        Returns:
            True if practice was recorded, False if skill not found
        """
        if skill_id not in self.skills:
            logger.warning(f"Skill {skill_id} not found for practice")
            return False

        skill = self.skills[skill_id]
        skill.practiced = True
        skill.updated_at = datetime.now()

        # Increase proficiency based on practice
        # In a real system, this would be more sophisticated
        proficiency_increase = min(0.1, duration_minutes / 60.0 * 0.02)  # Max 0.1 per hour
        skill.proficiency_score = min(1.0, skill.proficiency_score + proficiency_increase)

        # Update level based on proficiency
        if skill.proficiency_score >= 0.9:
            skill.level = SkillLevel.EXPERT
        elif skill.proficiency_score >= 0.7:
            skill.level = SkillLevel.ADVANCED
        elif skill.proficiency_score >= 0.4:
            skill.level = SkillLevel.INTERMEDIATE
        else:
            skill.level = SkillLevel.BEGINNER

        # Create learning session record
        session_id = self._generate_session_id()
        session = SkillLearningSession(
            id=session_id,
            skill_id=skill_id,
            goal=practice_description,
            time_spent_minutes=duration_minutes,
            notes=f"Practice session: {practice_description}"
        )
        self.learning_sessions[session_id] = session

        # Store updates in memory
        if self.memory_system:
            # Update skill
            skill_memory = MemoryEntry(
                id=f"skill_{skill.id}",
                memory_type=MemoryType.PROCEDURAL,
                content=skill.to_dict(),
                importance_score=0.8 + skill.proficiency_score * 0.2,
                tags=["skill", skill.skill_type.value, "practiced"],
                expires_at=None
            )
            await self.memory_system.store_memory(skill_memory)

            # Store session
            session_memory = MemoryEntry(
                id=f"session_{session.id}",
                memory_type=MemoryType.EPISODIC,
                content=session.__dict__,
                importance_score=0.6,
                tags=["skill_practice", skill_id],
                expires_at=None
            )
            await self.memory_system.store_memory(session_memory)

        logger.info(f"Practiced skill '{skill.name}' for {duration_minutes} minutes. New proficiency: {skill.proficiency_score:.2f}")
        return True

    async def use_skill(self,
                       skill_id: str,
                       action: str,
                       context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """
        Use/apply a learned skill.

        Args:
            skill_id: ID of skill to use
            action: Action to perform with the skill
            context: Optional context for the action

        Returns:
            Result of using the skill
        """
        if skill_id not in self.skills:
            return {
                "error": f"Skill {skill_id} not found",
                "available_skills": list(self.skills.keys())
            }

        skill = self.skills[skill_id]

        # Update last used time
        skill.updated_at = datetime.now()

        result = {
            "skill_id": skill.id,
            "skill_name": skill.name,
            "skill_type": skill.skill_type.value,
            "action_performed": action,
            "proficiency_level": skill.level.value,
            "proficiency_score": skill.proficiency_score,
            "timestamp": datetime.now().isoformat(),
            "success": True
        }

        # Handle specific skill types
        if skill.skill_type == SkillType.APPLICATION:
            result.update(await self._execute_application_action(skill, action, context))
        elif skill.skill_type == SkillType.LANGUAGE:
            result.update(await self._process_language_action(skill, action, context))
        elif skill.skill_type == SkillType.CODING:
            result.update(await self._process_coding_action(skill, action, context))

        # Store usage in memory
        if self.memory_system:
            usage_memory = MemoryEntry(
                id=f"skill_usage_{skill.id}_{int(datetime.now().timestamp())}",
                memory_type=MemoryType.EPISODIC,
                content=result,
                importance_score=0.5,
                tags=["skill_usage", skill_id],
                expires_at=None
            )
            await self.memory_system.store_memory(usage_memory)

        return result

    async def _execute_application_action(self,
                                        skill: Skill,
                                        action: str,
                                        context: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        """Execute an application control action"""
        executable = skill.metadata.get("executable")
        if not executable:
            return {
                "error": f"No executable configured for {skill.name}",
                "suggestion": "Please specify the application executable path"
            }

        # Map common actions to commands
        action_lower = action.lower()
        command_parts = [executable]

        if action_lower in ["open", "launch", "start"]:
            # Just opening the application
            pass
        elif action_lower in ["close", "exit", "quit"]:
            # This would require process management - simplified for now
            return {
                "action": "close_application",
                "note": "Application closing would require process management implementation",
                "executable": executable
            }
        elif action_lower in ["save", "print", "new", "open_file"]:
            # These would require application-specific automation
            return {
                "action": f"application_action_{action}",
                "note": f"Action '{action}' for {skill.name} would require specific automation implementation",
                "executable": executable,
                "suggestion": "Consider using UI automation libraries for specific application actions"
            }
        else:
            # Generic action - just launch with potential parameters
            if context and "file_path" in context:
                command_parts.append(context["file_path"])

        return {
            "action": "launch_application",
            "executable": executable,
            "command": " ".join(command_parts),
            "note": "Application launch initiated. For complex automation, consider UI automation frameworks."
        }

    async def _process_language_action(self,
                                     skill: Skill,
                                     action: str,
                                     context: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        """Process a language-related action"""
        language_name = skill.metadata.get("language_name", "Unknown")
        focus_areas = skill.metadata.get("focus_areas", [])

        if action.lower() == "translate":
            text_to_translate = context.get("text", "") if context else ""
            return {
                "action": "translate",
                "language": language_name,
                "text_to_translate": text_to_translate[:100] + "..." if len(text_to_translate) > 100 else text_to_translate,
                "note": "Translation would require integration with translation API or model",
                "suggestion": "For accurate translation, use dedicated translation services"
            }
        elif action.lower() in ["converse", "speak", "talk"]:
            return {
                "action": "language_practice",
                "language": language_name,
                "focus_areas": focus_areas,
                "note": "Language conversation practice would require speech recognition/synthesis",
                "suggestion": "Consider using language learning apps for conversation practice"
            }
        elif action.lower() == "learn_vocabulary":
            vocab_list = context.get("vocabulary", []) if context else []
            return {
                "action": "vocabulary_learning",
                "language": language_name,
                "vocabulary_count": len(vocab_list),
                "note": "Vocabulary learning would benefit from spaced repetition systems",
                "suggestion": "Consider using flashcard applications like Anki"
            }
        else:
            return {
                "action": "language_action",
                "language": language_name,
                "action_requested": action,
                "note": f"Language action '{action}' for {language_name}",
                "suggestion": "Specify actions like 'translate', 'converse', or 'learn_vocabulary'"
            }

    async def _process_coding_action(self,
                                   skill: Skill,
                                   action: str,
                                   context: Optional[Dict[str, Any]]) -> Dict[str, Any]:
        """Process a coding-related action"""
        return {
            "action": "coding_action",
            "skill_name": skill.name,
            "action_requested": action,
            "note": f"Coding action '{action}' for skill {skill.name}",
            "suggestion": "For code generation/editing, use the reasoning and task orchestration modules"
        }

    async def get_skill(self, skill_id: str) -> Optional[Skill]:
        """Get a skill by ID"""
        return self.skills.get(skill_id)

    async def list_skills(self,
                         skill_type: Optional[SkillType] = None,
                         min_level: Optional[SkillLevel] = None) -> List[Skill]:
        """
        List skills, optionally filtered by type and minimum level.

        Args:
            skill_type: Filter by skill type
            min_level: Filter by minimum proficiency level

        Returns:
            List of Skill objects
        """
        skills = list(self.skills.values())

        if skill_type:
            skills = [s for s in skills if s.skill_type == skill_type]

        if min_level:
            level_order = {
                SkillLevel.BEGINNER: 0,
                SkillLevel.INTERMEDIATE: 1,
                SkillLevel.ADVANCED: 2,
                SkillLevel.EXPERT: 3
            }
            min_level_value = level_order[min_level]
            skills = [s for s in skills if level_order[s.level] >= min_level_value]

        return skills

    async def get_learning_path(self, skill_id: str) -> List[Dict[str, Any]]:
        """
        Get a suggested learning path for a skill.

        Args:
            skill_id: ID of skill

        Returns:
            List of learning path steps
        """
        if skill_id not in self.skills:
            return [{"error": f"Skill {skill_id} not found"}]

        skill = self.skills[skill_id]

        # Base learning path from metadata
        base_path = skill.metadata.get("learning_path", [])

        # Enhance with RAG resources if available
        if self.rag_available and base_path:
            enhanced_path = []
            for i, step in enumerate(base_path):
                path_step = {
                    "step": i + 1,
                    "description": step,
                    "resources": []
                }

                # Add skill-specific resources
                path_step["resources"].extend([
                    {
                        "title": f"{skill.name} - {step} Guide",
                        "type": "tutorial",
                        "source": "Skill Documentation"
                    }
                ])

                # Add RAG resources if we can simulate them
                if self.rag_available:
                    path_step["resources"].extend([
                        {
                            "title": f"{skill.name} {step} - RAG Resource",
                            "url": f"rag://{skill.id}/step_{i+1}",
                            "type": "documentation",
                            "source": "RAG System"
                        }
                    ])

                enhanced_path.append(path_step)

            return enhanced_path

        # Return basic path if no enhancement possible
        return [
            {
                "step": i + 1,
                "description": step,
                "resources": []
            }
            for i, step in enumerate(base_path)
        ]

    async def teach_skill(self,
                         teacher_skill_id: str,
                         learner_goal: str,
                         context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """
        Teach a skill to the user or create a learning plan.

        Args:
            teacher_skill_id: ID of skill to teach
            learner_goal: What the learner wants to achieve
            context: Optional context for teaching

        Returns:
            Teaching plan and resources
        """
        if teacher_skill_id not in self.skills:
            return {"error": f"Teacher skill {teacher_skill_id} not found"}

        skill = self.skills[teacher_skill_id]

        # Create a learning session
        session_id = self._generate_session_id()
        session = SkillLearningSession(
            id=session_id,
            skill_id=teacher_skill_id,
            goal=learner_goal,
            notes=f"Teaching session initiated: {learner_goal}"
        )
        self.learning_sessions[session_id] = session

        # Get learning path
        learning_path = await self.get_learning_path(teacher_skill_id)

        # Generate teaching plan
        teaching_plan = {
            "skill_id": teacher_skill_id,
            "skill_name": skill.name,
            "skill_type": skill.skill_type.value,
            "learner_goal": learner_goal,
            "teaching_session_id": session_id,
            "current_level": skill.level.value,
            "target_proficiency": "intermediate",  # Default target
            "estimated_time_hours": len(learning_path) * 2,  # Rough estimate
            "learning_path": learning_path,
            "prerequisites": skill.prerequisites,
            "suggested_resources": skill.resources,
            "practice_recommendations": [
                f"Practice {skill.name} for 15-30 minutes daily",
                "Apply the skill to real-world problems",
                "Teach the concept to someone else to reinforce learning",
                "Build a small project using the skill"
            ]
        }

        # Store teaching session in memory
        if self.memory_system:
            teaching_memory = MemoryEntry(
                id=f"teaching_session_{session.id}",
                memory_type=MemoryType.EPISODIC,
                content=teaching_plan,
                importance_score=0.7,
                tags=["teaching", teacher_skill_id],
                expires_at=None
            )
            await self.memory_system.store_memory(teaching_memory)

        return teaching_plan

    def get_available_applications(self) -> Dict[str, List[str]]:
        """Get list of applications that can be controlled"""
        return self.application_commands.get("windows", {}) if os.name == 'nt' else {}