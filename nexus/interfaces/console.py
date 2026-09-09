"""
Console interface for NEXUS
Provides a text-based interactive experience
"""

import asyncio
import sys
from typing import Optional
import logging
from datetime import datetime

# Import NEXUS components
from ..core.reasoning import ReasoningEngine
from ..core.context import ContextManager
from ..core.memory import MemorySystem
from ..modules.task_orchestration import TaskOrchestrator
from ..modules.information_synthesis import InformationSynthesizer
from ..modules.creative_collaboration import CreativeCollaborator
from ..modules.decision_support import DecisionAnalyzer
from ..modules.specialized_modes import SpecializedModes, ModeType
from ..modules.skill_learning import SkillLearner, SkillType, SkillLevel

logger = logging.getLogger(__name__)


class NexusConsole:
    """
    Console-based interface for interacting with NEXUS.
    """

    def __init__(self):
        # Initialize core components
        self.memory_system = MemorySystem("nexus_console_memory.db")
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

        # Initialize skill learning system
        self.skill_learner = SkillLearner(
            memory_system=self.memory_system,
            rag_path=r"D:\Desktop\RAG"  # Using the RAG path mentioned by the user
        )

        # Initialize context
        self.session_id = f"console_{datetime.now().timestamp()}"
        self.is_running = False

        # Display settings
        self.show_tokens = False
        self.verbose = False

    async def initialize(self) -> None:
        """Initialize the console interface"""
        # Initialize session context
        await self.context_manager.initialize_session(self.session_id)

        logger.info("NEXUS Console initialized")

    def display_welcome(self) -> None:
        """Display welcome message"""
        welcome = """
+-------------------------------------------------------------------------------+
|                                                                               |
|    N E X U S                                                                  |
|                                                                               |
|                  Neural Executive Understanding System                        |
|                                                                               |
|    Type 'help' for commands, or just start talking to NEXUS!                 |
|                                                                               |
+-------------------------------------------------------------------------------+
        """
        print(welcome)

    def display_help(self) -> None:
        """Display help information"""
        help_text = """
NEXUS Commands:
  help                    - Show this help message
  exit, quit              - Exit NEXUS
  status                  - Show current system status
  modes                   - List active specialized modes
  activate <mode>         - Activate a specialized mode (deep_focus, learning_companion, creative_collaboration, strategic_planning)
  deactivate <mode>       - Deactivate a specialized mode
  context                 - Show current context
  memory                  - Show memory statistics
  clear                   - Clear the screen
  settings                - Show or change settings
  skill learn <name>      - Learn a new skill (e.g., 'skill learn Python programming')
  skill language <lang>   - Learn a new language (e.g., 'skill language Spanish')
  skill app <app>         - Learn to control an application (e.g., 'skill app notepad')
  skill list              - List all learned skills
  skill info <id>         - Get information about a specific skill
  skill use <id> <action> - Use a learned skill (e.g., 'skill use skill_001 open')
  skill practice <id>     - Practice a skill to improve proficiency

Specialized Modes:
  deep_focus              - For concentrated, uninterrupted work
  learning_companion      - For acquiring new knowledge or skills
  creative_collaboration  - For brainstorming and idea generation
  strategic_planning      - For long-term planning and decision making

Examples:
  > activate deep_focus
  > I need to finish this report by 5 PM
  > deactivate deep_focus
  > activate learning_companion
  > Teach me about quantum computing
  > What are the key principles?
  > skill learn Python programming
  > skill language Japanese
  > skill app chrome
  > skill list
  > skill use skill_001 open

Just talk naturally - NEXUS will adapt to your needs!
        """
        print(help_text)

    async def process_input(self, user_input: str) -> str:
        """
        Process user input and generate a response.

        Args:
            user_input: The user's input string

        Returns:
            NEXUS's response string
        """
        user_input = user_input.strip()
        if not user_input:
            return "I'm here when you're ready to talk."

        # Handle commands
        if user_input.lower() in ['exit', 'quit']:
            self.is_running = False
            return "Goodbye! NEXUS signing off."

        if user_input.lower() == 'help':
            self.display_help()
            return ""  # Empty string since help was already displayed

        if user_input.lower() == 'status':
            return await self.get_status()

        if user_input.lower() == 'modes':
            return await self.get_modes_status()

        if user_input.lower().startswith('activate '):
            mode_name = user_input[9:].strip()
            return await self.activate_mode(mode_name)

        if user_input.lower().startswith('deactivate '):
            mode_name = user_input[11:].strip()
            return await self.deactivate_mode(mode_name)

        if user_input.lower() == 'context':
            return await self.get_context()

        if user_input.lower() == 'memory':
            return await self.get_memory_stats()

        if user_input.lower() == 'clear':
            # Clear screen (works on most terminals)
            print("\033[2J\033[H", end="")
            return ""

        if user_input.lower() == 'settings':
            return await self.get_settings()

        # Handle skill learning commands
        if user_input.lower().startswith('skill '):
            return await self.handle_skill_command(user_input[6:].strip())

        # Check if input is a mode activation request in natural language
        activated_mode = await self._check_for_mode_request(user_input)
        if activated_mode:
            # Activate the mode and process the input within that mode
            await self.activate_mode(activated_mode)
            result = await self.specialized_modes.process_in_mode(
                activated_mode, user_input
            )
            return result.get("response", "I've activated the mode and processed your request.")

        # Determine which mode(s) are active and route accordingly
        active_modes = self.specialized_modes.get_active_modes()
        if active_modes:
            # If multiple modes are active, prioritize the most recently activated
            # For simplicity, we'll use the first active mode (could be improved)
            if active_modes:
                mode_type_str = active_modes[0]["mode_type"]
                mode_type = ModeType(mode_type_str)
                result = await self.specialized_modes.process_in_mode(
                    mode_type, user_input
                )
                return result.get("response", "I've processed your request in the active mode.")

        # No active modes - use general reasoning
        # Update context
        await self.context_manager.update_context(
            user_input,
            {"placeholder": "processing"},
            {}
        )

        # Use reasoning engine to process
        reasoning_result = await self.reasoning_engine.process_query(
            user_input,
            context=self.context_manager.get_current_context()
        )

        # Update context with the interaction
        await self.context_manager.update_context(
            user_input,
            reasoning_result
        )

        return reasoning_result["response"]

    async def _check_for_mode_request(self, user_input: str) -> Optional[ModeType]:
        """
        Check if user input is requesting to activate a specific mode.

        Args:
            user_input: User's input string

        Returns:
            ModeType if a mode request is detected, None otherwise
        """
        input_lower = user_input.lower()

        # Deep focus triggers
        if any(phrase in input_lower for phrase in ["deep focus", "focus mode", "concentrate", "no distractions", "need to focus"]):
            return ModeType.DEEP_FOCUS

        # Learning companion triggers
        if any(phrase in input_lower for phrase in ["learn", "teach me", "study", "understand", "explain"]):
            return ModeType.LEARNING_COMPANION

        # Creative collaboration triggers
        if any(phrase in input_lower for phrase in ["brainstorm", "ideas", "creative", "innovate", "think of"]):
            return ModeType.CREATIVE_COLLABORATION

        # Strategic planning triggers
        if any(phrase in input_lower for phrase in ["plan", "strategy", "long-term", "goal", "objective"]):
            return ModeType.STRATEGIC_PLANNING

        return None

    async def get_status(self) -> str:
        """Get current system status"""
        context = self.context_manager.get_current_context()
        active_modes = self.specialized_modes.get_active_modes()

        status = f"""
NEXUS Status:
-------------
Session ID: {self.session_id}
Uptime: {self._get_uptime()}
Active Modes: {len(active_modes)}
Memory Entries: {self.memory_system.get_memory_stats().get('total_memories', 0)}
Context: {context.get('session_context', {}).get('emotional_state', 'unknown')} emotional state,
          {context.get('session_context', {}).get('cognitive_load', 'unknown')} cognitive load
        """
        return status.strip()

    async def get_modes_status(self) -> str:
        """Get status of specialized modes"""
        active_modes = self.specialized_modes.get_active_modes()

        if not active_modes:
            return "No active modes. Activate a mode with 'activate <mode_name>'"

        status = "Active Modes:\n-------------\n"
        for mode in active_modes:
            duration = (datetime.now() - datetime.fromisoformat(mode["activated_at"])).total_seconds() / 60
            status += f"• {mode['mode_type']} - Active for {duration:.1f} minutes\n"
            if mode.get('user_goal'):
                status += f"  Goal: {mode['user_goal']}\n"

        return status.strip()

    async def activate_mode(self, mode_name: str) -> str:
        """Activate a specialized mode"""
        try:
            mode_type = ModeType(mode_name.lower())
            mode_context = await self.specialized_modes.activate_mode(mode_type)

            return f"Activated {mode_type.value} mode. {self._get_mode_help(mode_type)}"
        except ValueError:
            available_modes = [m.value for m in ModeType]
            return f"Unknown mode: {mode_name}. Available modes: {', '.join(available_modes)}"

    async def deactivate_mode(self, mode_name: str) -> str:
        """Deactivate a specialized mode"""
        try:
            mode_type = ModeType(mode_name.lower())
            success = await self.specialized_modes.deactivate_mode(mode_type)

            if success:
                return f"Deactivated {mode_type.value} mode."
            else:
                return f"Mode {mode_type.value} was not active."
        except ValueError:
            available_modes = [m.value for m in ModeType]
            return f"Unknown mode: {mode_name}. Available modes: {', '.join(available_modes)}"

    async def get_context(self) -> str:
        """Get current context"""
        context = self.context_manager.get_current_context()

        # Format context for display
        ctx_str = "Current Context:\n---------------\n"

        # User profile
        profile = context.get('user_profile', {})
        if profile:
            ctx_str += f"User: {profile.get('name', 'Unknown')}\n"
            ctx_str += f"Expertise: {', '.join(profile.get('expertise_areas', [])) or 'None specified'}\n"
            ctx_str += f"Goals: {', '.join(profile.get('goals', [])) or 'None specified'}\n"

        # Session context
        session = context.get('session_context', {})
        if session:
            ctx_str += f"\nSession:\n"
            ctx_str += f"  Duration: {session.get('duration_minutes', 0):.1f} minutes\n"
            ctx_str += f"  Task: {session.get('current_task', 'None')}\n"
            ctx_str += f"  Emotional State: {session.get('emotional_state', 'neutral')}\n"
            ctx_str += f"  Cognitive Load: {session.get('cognitive_load', 'medium')}\n"
            ctx_str += f"  Energy Level: {session.get('energy_level', 'medium')}\n"

        # Active projects
        projects = context.get('active_projects', [])
        if projects:
            ctx_str += f"\nActive Projects ({len(projects)}):\n"
            for proj in projects[:3]:  # Show first 3
                ctx_str += f"  • {proj.get('name', 'Unnamed')} ({proj.get('progress', 0)*100:.0f}% complete)\n"

        return ctx_str.strip()

    async def get_memory_stats(self) -> str:
        """Get memory statistics"""
        stats = self.memory_system.get_memory_stats()

        if "error" in stats:
            return f"Memory error: {stats['error']}"

        mem_str = "Memory Statistics:\n------------------\n"
        mem_str += f"Total Memories: {stats.get('total_memories', 0)}\n"

        type_counts = stats.get('type_counts', {})
        if type_counts:
            mem_str += "By Type:\n"
            for mem_type, count in type_counts.items():
                mem_str += f"  {mem_type}: {count}\n"

        mem_str += f"Average Importance: {stats.get('average_importance', 0):.2f}\n"
        mem_str += f"Cache Size: {stats.get('cache_size', 0)} entries\n"

        return mem_str.strip()

    async def get_settings(self) -> str:
        """Get or show settings"""
        # For now, just show current settings
        settings_str = "NEXUS Settings:\n-------------\n"
        settings_str += f"Verbose: {self.verbose}\n"
        settings_str += f"Show Tokens: {self.show_tokens}\n"
        settings_str += f"Memory DB: nexus_console_memory.db\n"
        settings_str += f"Session ID: {self.session_id}\n"
        return settings_str

    async def handle_skill_command(self, command: str) -> str:
        """Handle skill learning commands"""
        command_parts = command.split()
        if not command_parts:
            return "Please specify a skill command. Type 'help' for available commands."

        cmd = command_parts[0].lower()

        if cmd == "learn":
            if len(command_parts) < 2:
                return "Please specify what skill to learn. Usage: skill learn <skill description>"

            skill_description = " ".join(command_parts[1:])
            skill = await self.skill_learner.learn_skill(
                name=skill_description,
                description=f"User-requested skill: {skill_description}",
                skill_type=SkillType.CODING,  # Default to coding, could be enhanced with NLP
                level=SkillLevel.BEGINNER
            )

            return f"I've started learning the skill: {skill.name}\n" \
                   f"Skill ID: {skill.id}\n" \
                   f"Type: {skill.skill_type.value}\n" \
                   f"Level: {skill.level.value}\n" \
                   f"You can practice this skill with: skill practice {skill.id}"

        elif cmd == "language":
            if len(command_parts) < 2:
                return "Please specify what language to learn. Usage: skill language <language name>"

            language_name = " ".join(command_parts[1:])
            skill = await self.skill_learner.learn_language(
                language_name=language_name,
                target_level=SkillLevel.INTERMEDIATE
            )

            return f"I've started helping you learn: {skill.name}\n" \
                   f"Skill ID: {skill.id}\n" \
                   f"Language: {skill.metadata.get('language_name', language_name)}\n" \
                   f"Target Level: {skill.level.value}\n" \
                   f"You can practice with: skill practice {skill.id}"

        elif cmd == "app":
            if len(command_parts) < 2:
                return "Please specify what application to learn. Usage: skill app <application name>"

            app_name = " ".join(command_parts[1:])
            skill = await self.skill_learner.learn_application_control(
                application_name=app_name
            )

            executable = skill.metadata.get("executable", "unknown")
            return f"I've started helping you learn to control: {skill.name}\n" \
                   f"Skill ID: {skill.id}\n" \
                   f"Application: {app_name}\n" \
                   f"Executable: {executable}\n" \
                   f"You can practice with: skill practice {skill.id}"

        elif cmd == "list":
            skills = await self.skill_learner.list_skills()
            if not skills:
                return "No skills learned yet. Use 'skill learn <description>' to start learning a skill."

            skill_list = "Learned Skills:\n"
            for skill in skills:
                skill_list += f"  {skill.id}: {skill.name} ({skill.skill_type.value}) - {skill.level.value}\n"

            return skill_list.strip()

        elif cmd == "info":
            if len(command_parts) < 2:
                return "Please specify a skill ID. Usage: skill info <skill_id>"

            skill_id = command_parts[1]
            skill = await self.skill_learner.get_skill(skill_id)
            if not skill:
                return f"Skill with ID '{skill_id}' not found. Use 'skill list' to see available skills."

            info = f"Skill Information:\n"
            info += f"  ID: {skill.id}\n"
            info += f"  Name: {skill.name}\n"
            info += f"  Description: {skill.description}\n"
            info += f"  Type: {skill.skill_type.value}\n"
            info += f"  Level: {skill.level.value}\n"
            info += f"  Proficiency: {skill.proficiency_score:.2f}\n"
            info += f"  Learned At: {skill.learned_at.strftime('%Y-%m-%d %H:%M:%S') if skill.learned_at else 'Unknown'}\n"

            if skill.resources:
                info += f"  Resources ({len(skill.resources)}):\n"
                for resource in skill.resources[:3]:  # Show first 3 resources
                    info += f"    - {resource.get('title', 'Unknown')} ({resource.get('type', 'unknown')})\n"
                if len(skill.resources) > 3:
                    info += f"    ... and {len(skill.resources) - 3} more\n"

            return info.strip()

        elif cmd == "use":
            if len(command_parts) < 3:
                return "Please specify a skill ID and action. Usage: skill use <skill_id> <action>"

            skill_id = command_parts[1]
            action = " ".join(command_parts[2:])

            result = await self.skill_learner.use_skill(skill_id, action)

            if "error" in result:
                return f"Error: {result['error']}"

            response = f"Used skill '{result['skill_name']}' ({result['skill_type']}) to perform: {result['action_performed']}\n"
            response += f"Proficiency: {result['proficiency_level']} ({result['proficiency_score']:.2f})\n"

            if result.get("executable"):
                response += f"Launched: {result['executable']}\n"
                if result.get("command"):
                    response += f"Command: {result['command']}\n"

            if result.get("note"):
                response += f"Note: {result['note']}\n"

            return response.strip()

        elif cmd == "practice":
            if len(command_parts) < 2:
                return "Please specify a skill ID to practice. Usage: skill practice <skill_id> [minutes]"

            skill_id = command_parts[1]
            duration = 15.0  # Default duration

            if len(command_parts) > 2:
                try:
                    duration = float(command_parts[2])
                except ValueError:
                    return "Invalid duration. Please specify a number of minutes."

            success = await self.skill_learner.practice_skill(
                skill_id=skill_id,
                practice_description=f"User requested practice session",
                duration_minutes=duration
            )

            if not success:
                return f"Skill with ID '{skill_id}' not found. Use 'skill list' to see available skills."

            skill = await self.skill_learner.get_skill(skill_id)
            return f"Practiced skill '{skill.name}' for {duration} minutes.\n" \
                   f"New proficiency: {skill.proficiency_score:.2f} ({skill.level.value})\n" \
                   f"Keep practicing to improve your mastery!"

        else:
            return f"Unknown skill command: '{cmd}'. Available commands: learn, language, app, list, info, use, practice"

    def _get_uptime(self) -> str:
        """Get system uptime"""
        # In a real implementation, we would track start time
        return "Session-based (reset on restart)"

    def _get_mode_help(self, mode_type: ModeType) -> str:
        """Get help text for a specific mode"""
        help_texts = {
            ModeType.DEEP_FOCUS: "I'll help you maintain focus by minimizing distractions and suggesting optimal break times.",
            ModeType.LEARNING_COMPANION: "I'll act as your learning partner, helping you acquire knowledge and skills effectively.",
            ModeType.CREATIVE_COLLABORATION: "I'll collaborate with you on creative tasks, brainstorming ideas and exploring possibilities.",
            ModeType.STRATEGIC_PLANNING: "I'll assist with long-term planning, goal setting, and strategic decision making."
        }
        return help_texts.get(mode_type, "")

    async def run(self) -> None:
        """Run the console interface"""
        self.is_running = True
        self.display_welcome()

        # Show initial help
        print("Type 'help' for available commands.\n")

        while self.is_running:
            try:
                # Get user input
                user_input = input("\n> ").strip()

                # Process input and get response
                response = await self.process_input(user_input)

                # Display response if not empty
                if response:
                    print(f"\n{response}")

            except KeyboardInterrupt:
                print("\n\nUse 'exit' to quit NEXUS.")
            except EOFError:
                # Handle Ctrl+D
                break
            except Exception as e:
                logger.error(f"Error in console loop: {e}")
                print(f"\nAn error occurred: {e}")
                print("Please try again or type 'exit' to quit.")

        print("\nThank you for using NEXUS!")


# Main entry point
async def main():
    """Main entry point for NEXUS Console"""
    console = NexusConsole()
    await console.initialize()
    await console.run()


if __name__ == "__main__":
    # Configure logging
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
    )

    # Run the console
    asyncio.run(main())