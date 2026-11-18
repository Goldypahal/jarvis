"""
Basic usage example for NEXUS
Demonstrates how to use NEXUS programmatically
"""
import asyncio
import sys
import os

# Add the nexus package to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from nexus.core.reasoning import ReasoningEngine
from nexus.core.context import ContextManager
from nexus.core.memory import MemorySystem
from nexus.modules.specialized_modes import SpecializedModes, ModeType

async def main():
    """Demonstrate basic NEXUS usage"""
    print("[NEXUS] Basic Usage Example")
    print("=" * 40)

    # Initialize core components
    memory = MemorySystem("example_memory.db")
    context = ContextManager(user_id="example_user")
    reasoning = ReasoningEngine(context)
    modes = SpecializedModes(
        reasoning_engine=reasoning,
        context_manager=context,
        memory_system=memory
    )

    # Initialize session
    await context.initialize_session("example_session_001")
    print("[OK] Initialized NEXUS components")

    # Example 1: Regular reasoning
    print("\n[Example 1] Regular Reasoning")
    print("-" * 30)
    query = "What are the benefits of regular exercise?"
    result = await reasoning.process_query(query)
    print(f"Query: {query}")
    print(f"Response: {result['response'][:100]}...")
    print(f"Reasoning Mode: {result['reasoning_mode']}")
    print(f"Confidence: {result['confidence']:.0%}")

    # Example 2: Activating a specialized mode
    print("\n[Example 2] Specialized Mode - Learning Companion")
    print("-" * 50)
    mode_context = await modes.activate_mode(ModeType.LEARNING_COMPANION, "Understand machine learning basics")
    print(f"[OK] Activated {mode_context.mode_type.value} mode")

    # Process input in the learning mode
    learning_query = "Can you explain the difference between supervised and unsupervised learning?"
    learning_result = await modes.process_in_mode(ModeType.LEARNING_COMPANION, learning_query)
    print(f"Query: {learning_query}")
    print(f"Response: {learning_result.get('response', 'No response')[:150]}...")
    if 'key_insights' in learning_result:
        print(f"Key Insights: {len(learning_result['key_insights'])} insights generated")

    # Example 3: Task orchestration
    print("\n[Example 3] Task Orchestration")
    print("-" * 30)
    from nexus.modules.task_orchestration import TaskOrchestrator
    orchestrator = TaskOrchestrator(memory)

    # Create a simple project
    project = await orchestrator.create_project(
        name="Learn Python",
        description="Project to learn Python programming",
        goals=["Learn Python syntax", "Build a simple application"],
        stakeholders=["Self"]
    )
    print(f"[OK] Created project: {project.name}")

    # Decompose a goal
    goal_result = await orchestrator.decompose_goal(
        goal="Build a simple calculator app in Python",
        timeframe="1 week",
        resources=["Computer", "Python tutorial"]
    )
    print(f"[OK] Decomposed goal into {goal_result['tasks_created']} initial tasks")
    print(f"  Estimated effort: {goal_result['estimated_total_effort']} hours")

    # Example 4: Memory usage
    print("\n[Example 4] Memory System")
    print("-" * 26)
    stats = memory.get_memory_stats()
    print(f"[OK] Memory Statistics:")
    print(f"  Total memories: {stats.get('total_memories', 0)}")
    print(f"  By type: {stats.get('type_counts', {})}")

    # Clean up
    import os
    memory.close()
    if os.path.exists("example_memory.db"):
        os.remove("example_memory.db")

    print("\n[SUCCESS] Example completed successfully!")
    print("Try running 'python -m nexus' to start the interactive console.")

if __name__ == "__main__":
    asyncio.run(main())