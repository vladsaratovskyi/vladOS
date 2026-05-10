# Agent Context

Before making roadmap or architecture decisions for this OS project, read
`GENERAL_PLAN.md` and keep its milestone order in mind.

Prioritize small, bootable steps. Do not jump ahead to filesystems,
multitasking, or userspace before the current memory, interrupts, and CPU setup
milestones are solid.

After implementing any new chunk of kernel functionality, keep the documentation
in sync before considering the work complete. Update both layers:

- high-level documentation, especially `README.md` and `docs/architecture.md`,
  so the current architecture, boot flow, test flow, and milestone status stay
  accurate.
- low-level walkthrough documentation under `docs/code_walkthrough/`, so new or
  changed source, test, and configuration code is explained line-by-line or in
  the smallest sensible code blocks.

Documentation updates should describe what changed, why it exists in the kernel,
how it fits into the current roadmap milestone, and which verification commands
prove it still boots or tests correctly.
