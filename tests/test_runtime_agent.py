"""
Integration tests for NEXUS Agent Runtime, State Machine, Verification, and Telemetry Replay.
"""

import asyncio
import os
import shutil
from nexus.models.schemas import (
    ModelMetadata,
    ModelCapability,
    ModelCapabilityScores,
    LatencyClass,
    PrivacyTier
)
from nexus.models.base import MockModelProvider
from nexus.models.registry import ModelCapabilityRegistry
from nexus.models.lifecycle import ModelLifecycleManager
from nexus.models.router import ModelRouter
from nexus.agent.tools.registry import ToolRegistry
from nexus.agent.tools.filesystem import ReadFileTool, WriteFileTool, ListDirTool
from nexus.agent.tools.git_tool import GitStatusTool
from nexus.agent.policy import PermissionPolicy, PermissionLevel
from nexus.telemetry.journal import ActionJournal
from nexus.telemetry.replay import TrajectoryReplayEngine
from nexus.core.runtime import AgentRuntime, AgentState


def test_agent_runtime_execution_and_journal():
    async def _run():
        test_dir = ".nexus_test_sandbox"
        os.makedirs(test_dir, exist_ok=True)
        test_file = os.path.join(test_dir, "hello.txt")

        try:
            # 1. Setup Models
            registry = ModelCapabilityRegistry()
            meta = ModelMetadata(
                name="nexus-personal-3b",
                provider_type="mock",
                capabilities=ModelCapability(
                    scores=ModelCapabilityScores(reasoning=3, coding=3),
                    latency_class=LatencyClass.FAST,
                    privacy=PrivacyTier.LOCAL
                )
            )
            mock_provider = MockModelProvider(meta)
            registry.register_provider(mock_provider)

            router = ModelRouter(registry)

            # 2. Setup Tools
            tool_reg = ToolRegistry()
            tool_reg.register(ReadFileTool())
            tool_reg.register(WriteFileTool())
            tool_reg.register(ListDirTool())
            tool_reg.register(GitStatusTool())

            # 3. Setup Journal & Runtime
            journal = ActionJournal(log_dir=os.path.join(test_dir, "trajectories"))
            policy = PermissionPolicy(default_level=PermissionLevel.LEVEL_2)
            runtime = AgentRuntime(router, tool_reg, policy=policy, journal=journal)

            states_visited = []
            runtime.set_state_listener(lambda st: states_visited.append(st))

            # 4. Process Request (Git status check)
            result = await runtime.process_request("Check git status", auto_mode=True)

            assert result.success is True
            assert len(result.observations) >= 1
            assert result.observations[0].tool_name == "git_status"
            assert result.observations[0].status == "success"

            # Verify that state machine transitioned through proper lifecycle
            assert AgentState.UNDERSTANDING in states_visited
            assert AgentState.PLANNING in states_visited
            assert AgentState.EXECUTING in states_visited
            assert AgentState.OBSERVING in states_visited
            assert AgentState.VERIFYING in states_visited
            assert AgentState.RESPOND in states_visited
            assert runtime.state == AgentState.IDLE

            # 5. Verify Action Journal
            runs = journal.list_runs()
            assert len(runs) == 1
            assert runs[0].run_id == result.run_id
            assert runs[0].tools_executed == ["git_status"]
            assert runs[0].success is True

            # 6. Test Trajectory Replay Engine
            replay_engine = TrajectoryReplayEngine(journal, tool_reg)
            replay_comparison = await replay_engine.replay_run(
                result.run_id,
                test_model_provider=mock_provider,
                auto_mode=True
            )
            assert replay_comparison.original_run_id == result.run_id
            assert replay_comparison.replay_success is True
            assert replay_comparison.plan_matched is True

        finally:
            if os.path.exists(test_dir):
                shutil.rmtree(test_dir)

    asyncio.run(_run())


def test_agent_verification_and_repair_loop():
    async def _run():
        test_dir = ".nexus_test_repair"
        os.makedirs(test_dir, exist_ok=True)

        try:
            registry = ModelCapabilityRegistry()
            meta = ModelMetadata(
                name="nexus-personal-3b",
                provider_type="mock",
                capabilities=ModelCapability()
            )
            mock_provider = MockModelProvider(meta)
            registry.register_provider(mock_provider)
            router = ModelRouter(registry)

            tool_reg = ToolRegistry()
            tool_reg.register(ReadFileTool())
            tool_reg.register(ListDirTool())

            journal = ActionJournal(log_dir=os.path.join(test_dir, "trajectories"))
            policy = PermissionPolicy(default_level=PermissionLevel.LEVEL_2)
            runtime = AgentRuntime(router, tool_reg, policy=policy, journal=journal)

            # Request to read a nonexistent file -> Trigger repair loop
            from nexus.models.schemas import NexusIntent, IntentType, ActionPlan, ActionStep, RiskLevel

            # Force an ActionPlan that fails on step 1 (reading nonexistent file)
            failing_intent = NexusIntent(
                raw_query="Read missing.txt",
                intent=IntentType.TASK_ORCHESTRATION,
                goal="Read missing.txt",
                requires_tools=True
            )

            # Plan with nonexistent file
            failing_plan = ActionPlan(
                plan_id="failing_plan",
                goal="Read missing.txt",
                steps=[
                    ActionStep(
                        step_id=1,
                        tool_name="read_file",
                        arguments={"path": "nonexistent_file_abc.txt"},
                        expected_outcome="Read contents",
                        risk_level=RiskLevel.LOW
                    )
                ]
            )

            # Planner returns this plan
            runtime.planner.create_plan = lambda intent, ctx=None: asyncio.sleep(0, result=failing_plan)

            result = await runtime.process_request("Read missing.txt", auto_mode=True)
            # The repair loop should have triggered and swapped step 1 to list_directory
            assert result.repairs_count >= 1
            assert len(result.observations) >= 1
            # Step 1 was repaired to list_directory and succeeded!
            last_obs = result.observations[-1]
            assert last_obs.tool_name == "list_directory"
            assert last_obs.status == "success"

        finally:
            if os.path.exists(test_dir):
                shutil.rmtree(test_dir)

    asyncio.run(_run())
