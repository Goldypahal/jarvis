"""
Dataset Builder for NEXUS-Personal Fine-Tuning.
Extracts successful interaction trajectories and user corrections from the ActionJournal
to build SFT and DPO training datasets formatted for Unsloth, Hugging Face TRL, or Axolotl.
"""

from typing import Any, Dict, List, Optional
import json
import os
import logging
from .journal import ActionJournal, TrajectoryRecord

logger = logging.getLogger(__name__)


class DatasetBuilder:
    """
    Builds training datasets for training the personal SLM (1B-3B)
    on personal intents, routing, tool calling, and preference corrections.
    """

    def __init__(self, journal: ActionJournal):
        self.journal = journal

    def build_sft_dataset(self, output_file: str = "dataset_sft.jsonl", min_success_only: bool = True) -> int:
        """
        Build instruction-tuning dataset from successful trajectory runs.
        Format: ShareGPT / Alpaca JSONL.
        """
        runs = self.journal.list_runs(limit=10000)
        examples = []

        for record in runs:
            if min_success_only and not record.success:
                continue

            # Build high-quality intent & tool execution example
            instruction = (
                "You are NEXUS-Personal, the personal assistant controller. "
                "Analyze the user query, determine the intent, plan the required tools, "
                "and generate structured action JSON."
            )
            input_text = record.user_request
            output_data = {
                "intent": record.intent,
                "plan": record.plan,
                "tools": record.tools_executed
            }

            examples.append({
                "instruction": instruction,
                "input": input_text,
                "output": json.dumps(output_data, indent=2)
            })

        os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
        with open(output_file, "w", encoding="utf-8") as f:
            for ex in examples:
                f.write(json.dumps(ex) + "\n")

        logger.info(f"Exported {len(examples)} SFT examples to {output_file}")
        return len(examples)

    def build_dpo_dataset(self, output_file: str = "dataset_dpo.jsonl") -> int:
        """
        Build Direct Preference Optimization (DPO) dataset.
        Identifies runs where repairs succeeded or where user corrections were provided:
        - prompt: original user request
        - chosen: repaired / corrected trajectory output
        - rejected: initial failed attempt output
        """
        runs = self.journal.list_runs(limit=10000)
        dpo_pairs = []

        for record in runs:
            # If the run required repairs to succeed, the initial plan vs final plan form a preference pair
            if record.repairs > 0 and record.success:
                prompt = record.user_request
                chosen_output = json.dumps({
                    "plan": record.plan,
                    "status": "success",
                    "repairs_needed": 0
                })
                rejected_output = json.dumps({
                    "observations": record.observations,
                    "status": "failed_initial_step"
                })

                dpo_pairs.append({
                    "prompt": prompt,
                    "chosen": chosen_output,
                    "rejected": rejected_output
                })

        os.makedirs(os.path.dirname(os.path.abspath(output_file)), exist_ok=True)
        with open(output_file, "w", encoding="utf-8") as f:
            for pair in dpo_pairs:
                f.write(json.dumps(pair) + "\n")

        logger.info(f"Exported {len(dpo_pairs)} DPO preference pairs to {output_file}")
        return len(dpo_pairs)
