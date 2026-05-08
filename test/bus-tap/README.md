# rollio-bus-tap

**Manual debugging tool**: subscribe to selected **camera**, **robot state**, and optional **robot command** topics for a fixed duration and print JSON summaries (frame counts, timestamps, episode status glimpses).

## CLI

Examples of flags: **`--camera name`** (repeatable), **`--robot-state name`**, **`--leader` / `--follower`** for command taps, **`--duration-s`**.

Useful to confirm publishers are alive and timestamps look sane before chasing issues in the full stack.

## See also

- [`rollio-bus`](../../rollio-bus/README.md) — service names tapped here.
