# rollio-teleop-router

**Leader → follower command forwarding** for teleoperation. One process per configured teleop pair.

## Behavior

- Subscribes to the **leader** channel’s state topics (joint, parallel gripper, or Cartesian pose depending on `TeleopRuntimeConfigV2`).
- Applies the configured **`MappingStrategy`** (e.g. direct joint index map, Cartesian mirror).
- Publishes **follower commands** on the matching command topic (joint position, MIT, parallel, end-pose, etc.).

The router deliberately **does not** own robot kinematics; FK/IK stay in device drivers. It implements **startup ramping** constants (documented in `src/lib.rs`) to limit how fast the follower converges when teleop engages.

## CLI

- **`rollio-teleop-router run`** with **`--config`** or **`--config-inline`** (teleop section slice for this pair).

## See also

- [`rollio-types`](../rollio-types/README.md) — `TeleopRuntimeConfigV2`, `MappingStrategy`.
- [`design/components.md`](../design/components.md) — conceptual teleop data flow.
