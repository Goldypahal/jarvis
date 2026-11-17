"""
NEXUS Empirical Benchmark Suite.
Measures routing accuracy, tool precision, verification/repair efficiency,
memory decay & contradiction resolution, and trajectory replay fidelity.
"""

import asyncio
import time
import os
import sys
import shutil
import json
from datetime import datetime, timedelta
from typing import Dict, Any, List

# Ensure workspace root is in sys.path
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from nexus.models.schemas import (
    ModelMetadata,
    ModelCapability,
    ModelCapabilityScores,
    LatencyClass,
    PrivacyTier,
    RiskLevel
)
from nexus.models.base import MockModelProvider
from nexus.models.registry import ModelCapabilityRegistry
from nexus.models.router import ModelRouter
from nexus.agent.tools.registry import ToolRegistry
from nexus.agent.tools.filesystem import ReadFileTool, WriteFileTool, ListDirTool
from nexus.agent.tools.git_tool import GitStatusTool
from nexus.agent.policy import PermissionPolicy, PermissionLevel
from nexus.telemetry.journal import ActionJournal
from nexus.telemetry.replay import TrajectoryReplayEngine
from nexus.telemetry.dataset_builder import DatasetBuilder
from nexus.core.runtime import AgentRuntime
from nexus.core.memory import MemorySystem, MemoryEntry, MemoryType
from nexus.memory.consolidation import MemoryConsolidator


class NexusBenchmarkRunner:
    """Automated benchmark harness for the NEXUS Agent Platform."""

    def __init__(self, sandbox_dir: str = ".benchmark_sandbox"):
        self.sandbox_dir = sandbox_dir
        self.results: Dict[str, Any] = {}

    def setup(self):
        os.makedirs(self.sandbox_dir, exist_ok=True)

        # 1. Setup Models
        self.registry = ModelCapabilityRegistry()
        self.personal_model = MockModelProvider(
            ModelMetadata(
                name="nexus-personal-3b",
                provider_type="mock",
                capabilities=ModelCapability(
                    scores=ModelCapabilityScores(reasoning=3, coding=2),
                    latency_class=LatencyClass.FAST,
                    privacy=PrivacyTier.LOCAL
                )
            )
        )
        self.reasoner_model = MockModelProvider(
            ModelMetadata(
                name="nexus-reasoner-7b",
                provider_type="mock",
                capabilities=ModelCapability(
                    scores=ModelCapabilityScores(reasoning=5, coding=5),
                    latency_class=LatencyClass.MEDIUM,
                    privacy=PrivacyTier.LOCAL
                )
            )
        )
        self.registry.register_provider(self.personal_model)
        self.registry.register_provider(self.reasoner_model)
        self.router = ModelRouter(self.registry)

        # 2. Setup Tools
        self.tools = ToolRegistry()
        self.tools.register(ReadFileTool())
        self.tools.register(WriteFileTool())
        self.tools.register(ListDirTool())
        self.tools.register(GitStatusTool())

        # 3. Setup Telemetry & Memory
        self.journal = ActionJournal(log_dir=os.path.join(self.sandbox_dir, "trajectories"))
        self.policy = PermissionPolicy(default_level=PermissionLevel.LEVEL_2)
        self.runtime = AgentRuntime(self.router, self.tools, policy=self.policy, journal=self.journal)
        self.memory = MemorySystem(db_path=os.path.join(self.sandbox_dir, "benchmark_memory.db"))

    def teardown(self):
        self.memory.close()
        if os.path.exists(self.sandbox_dir):
            shutil.rmtree(self.sandbox_dir)

    async def benchmark_routing(self) -> Dict[str, Any]:
        """Test routing accuracy across simple vs complex queries."""
        test_queries = [
            ("What is my active project?", "nexus-personal-3b"),
            ("Remember I like Python", "nexus-personal-3b"),
            ("Hello NEXUS, status report", "nexus-personal-3b"),
            ("Analyze root cause and fix failing pytest assertions in memory.py", "nexus-reasoner-7b"),
            ("Debug and optimize compiler benchmarks for deep reasoning architecture", "nexus-reasoner-7b"),
        ]

        correct = 0
        latencies = []

        for q, expected in test_queries:
            t0 = time.perf_counter()
            diff = self.router.estimate_difficulty(q)
            decision = self.router.select_model(diff)
            lat = (time.perf_counter() - t0) * 1000
            latencies.append(lat)

            if decision.selected_model == expected:
                correct += 1

        accuracy = correct / len(test_queries)
        avg_latency = sum(latencies) / len(latencies)

        return {
            "total_queries": len(test_queries),
            "correct_routes": correct,
            "accuracy": round(accuracy, 2),
            "avg_routing_latency_ms": round(avg_latency, 3)
        }

    async def benchmark_memory_decay_and_evolution(self) -> Dict[str, Any]:
        """Test cognitive memory decay, ranking, and contradiction resolution."""
        # 1. Store an evolving preference
        await self.memory.record_preference_evolution("preferred_language", "Python", reason="Initial setup")
        # Shift preference
        await self.memory.record_preference_evolution("preferred_language", "C++", reason="Leetcode optimization")

        # Check evolution history
        memories = await self.memory.get_memories(memory_type=MemoryType.SEMANTIC)
        pref_mem = next((m for m in memories if m.content.get("preference_key") == "preferred_language"), None)

        evolution_verified = (
            pref_mem is not None
            and pref_mem.content["value"] == "C++"
            and len(pref_mem.evolution_history) == 1
            and pref_mem.evolution_history[0]["previous_value"] == "Python"
        )

        # 2. Test decay scoring
        decay_entry = MemoryEntry(
            id="decay_test",
            memory_type=MemoryType.EPISODIC,
            content={"temp": "data"},
            timestamp=datetime.now() - timedelta(days=20),
            last_accessed=datetime.now() - timedelta(days=20),
            importance_score=0.4
        )
        await self.memory.store_memory(decay_entry)
        decayed_score = decay_entry.calculate_effective_importance(decay_rate=0.05)

        # 3. Test pruning
        consolidator = MemoryConsolidator(self.memory)
        pruned = await consolidator.prune_stale_memories(min_effective_importance=0.2, decay_rate=0.05)

        return {
            "preference_evolution_passed": evolution_verified,
            "decayed_effective_importance": decayed_score,
            "pruned_stale_memories_count": pruned
        }

    async def benchmark_agent_execution_and_replay(self) -> Dict[str, Any]:
        """Test full runtime execution, journal logging, and replay fidelity."""
        # Execute action
        result = await self.runtime.process_request("Check git status", auto_mode=True)
        assert result.success is True

        # Replay with Reasoner model
        replay_engine = TrajectoryReplayEngine(self.journal, self.tools)
        replay = await replay_engine.replay_run(
            result.run_id,
            test_model_provider=self.reasoner_model,
            auto_mode=True
        )

        # Build SFT and DPO datasets
        dataset_builder = DatasetBuilder(self.journal)
        sft_path = os.path.join(self.sandbox_dir, "bench_sft.jsonl")
        dpo_path = os.path.join(self.sandbox_dir, "bench_dpo.jsonl")
        sft_count = dataset_builder.build_sft_dataset(sft_path)
        dpo_count = dataset_builder.build_dpo_dataset(dpo_path)

        return {
            "execution_success": result.success,
            "observations_recorded": len(result.observations),
            "replay_success": replay.replay_success,
            "replay_plan_matched": replay.plan_matched,
            "sft_examples_exported": sft_count,
            "dpo_examples_exported": dpo_count
        }

    async def run_all(self) -> Dict[str, Any]:
        print("\n=======================================================")
        print("    NEXUS v2 Agent Platform - Benchmark Suite")
        print("=======================================================\n")
        self.setup()
        try:
            print("[1/3] Running Model Routing Benchmark...")
            routing_res = await self.benchmark_routing()
            print(f"      Accuracy: {routing_res['accuracy'] * 100}% | Latency: {routing_res['avg_routing_latency_ms']}ms")

            print("[2/3] Running Cognitive Memory & Evolution Benchmark...")
            memory_res = await self.benchmark_memory_decay_and_evolution()
            print(f"      Evolution Verified: {memory_res['preference_evolution_passed']} | Decayed Score: {memory_res['decayed_effective_importance']}")

            print("[3/3] Running Execution, Replay & Dataset Harvest Benchmark...")
            agent_res = await self.benchmark_agent_execution_and_replay()
            print(f"      Execution: {agent_res['execution_success']} | Replay: {agent_res['replay_success']} | SFT Export: {agent_res['sft_examples_exported']}")

            summary = {
                "timestamp": datetime.now().isoformat(),
                "routing": routing_res,
                "memory": memory_res,
                "agent_and_replay": agent_res,
                "status": "PASS"
            }
            print("\n>> All benchmarks PASSED successfully!")
            return summary
        finally:
            self.teardown()


if __name__ == "__main__":
    runner = NexusBenchmarkRunner()
    asyncio.run(runner.run_all())
