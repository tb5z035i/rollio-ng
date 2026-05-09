# rollio-monitor

Future **health / metrics** hub: watch child processes or bus traffic, compare against `ProjectConfig` thresholds, and surface warnings in the UI (described at a high level in [`design/components.md`](../design/components.md)).

---

## Concepts & behaviors (today)

The crate is **wired in the workspace** but the **binary is a stub**: it prints its package name and exits. **`rollio`** does **not** spawn it yet.

There is **no CLI surface** beyond that print — treat this README as intent, not shipped behavior.

---

## iceoryx2

**None today.** A future implementation would likely subscribe to metric services (TBD) rather than raw episode data.

---

## Lifecycle

Not started by orchestration.

---

## Built product & dependencies

**Binary:** `rollio-monitor` (stub). `Cargo.toml` already lists `iceoryx2` / `rollio-types` for upcoming work.

## See also

- [`design/components.md`](../design/components.md).
