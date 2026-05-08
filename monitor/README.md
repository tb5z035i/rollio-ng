# rollio-monitor

**Health and metrics aggregation** is planned in [`design/components.md`](../design/components.md) (thresholds, warnings, forwarding to the UI).

## Current status

The binary is a **stub**: it prints the package name and exits. The monitor process is **not** yet part of the functional pipeline.

When implemented, it should subscribe to per-process metrics on iceoryx2 and evaluate them against `ProjectConfig` monitor thresholds.
