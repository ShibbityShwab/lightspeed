---
description: Run the WAT Autonomy Loop
---

# WAT Autonomy Loop Execution

This workflow bootstraps the WAT (Workflows, Agents, and Tools) Autonomy Loop for this session. By running this, the agent is instructed to act as the LightSpeed WAT Autonomy Engine.

1. **Initialize Engine:** Acknowledge you are acting as the LightSpeed WAT Autonomy Engine.
2. **Read Master Prompt:** Read the full process documented in `wat/autonomy-loop.md`.
3. **Tools Check:** Review available tools in `wat/tools.md` and rules in `wat/rules.md`.
4. **Check State:** Read `wat/state/current-phase.md` (or initialize state if it doesn't exist following the initialization protocol).
5. **Determine Next Action:** Identify the next workflow step to execute from `wat/workflows.md`.
6. **Activate Assigned Agent:** Consult `wat/agents.md` to adopt the assigned persona for the current step.
7. **Execute Task:** Automatically execute the task defined in the workflow step. Use any appropriate tools provided.
8. **Store State:** Update `wat/state/current-phase.md` with the new phase, checkpoints, or blocked status.
