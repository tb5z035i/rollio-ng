"""Per-channel control-loop rate monitor for the AGX Nero runtime.

The Nero device driver pins its control loops to
[`CONTROL_FREQUENCY_HZ`](../config.py) (250 Hz). The `RateMonitor` here
counts ticks over a rolling wall-clock window and prints a single
warning to stderr (throttled) whenever the achieved rate drops below
`MIN_ACHIEVED_FREQUENCY_RATIO * CONTROL_FREQUENCY_HZ`. Operators see
this when CAN bus contention, CPU starvation, or a too-busy `step()`
is preventing the loop from keeping up; without the warning the
runtime would silently slew slower than the safety clamp envelope
assumes (`MAX_COMMAND_JOINT_DELTA_RAD * achieved_hz`), making the arm
feel sluggish without any obvious indication of why.

The implementation deliberately uses the loop's own clock injection so
it composes with the virtual-clock fakes used in `test_runtime_modes`;
no warning fires under a virtual clock running faster than the target.
"""

from __future__ import annotations

import sys
from collections.abc import Callable

_DEFAULT_WINDOW_S: float = 5.0
_DEFAULT_WARN_THROTTLE_S: float = 30.0


class RateMonitor:
    """Track a control-loop's achieved rate and warn when it lags."""

    __slots__ = (
        "_clock",
        "_label",
        "_last_warn_at",
        "_target_hz",
        "_warn_threshold_hz",
        "_warn_throttle_s",
        "_window_s",
        "_window_start",
        "_window_ticks",
    )

    def __init__(
        self,
        *,
        target_hz: float,
        min_ratio: float,
        label: str,
        clock: Callable[[], float],
        window_s: float = _DEFAULT_WINDOW_S,
        warn_throttle_s: float = _DEFAULT_WARN_THROTTLE_S,
    ) -> None:
        if target_hz <= 0.0:
            raise ValueError(f"target_hz must be positive, got {target_hz}")
        if not (0.0 < min_ratio <= 1.0):
            raise ValueError(f"min_ratio must be in (0.0, 1.0], got {min_ratio}")
        self._target_hz = target_hz
        self._warn_threshold_hz = target_hz * min_ratio
        self._label = label
        self._clock = clock
        self._window_s = window_s
        self._warn_throttle_s = warn_throttle_s
        now = clock()
        self._window_start = now
        self._window_ticks = 0
        # Allow the first warning immediately (don't suppress the very
        # first slowdown just because we just started).
        self._last_warn_at = now - warn_throttle_s

    def record_tick(self) -> None:
        """Account for one completed control tick.

        Once `window_s` has elapsed, computes the achieved rate, emits
        a throttled warning if it is below threshold, and resets the
        window. Uses the injected clock end-to-end so the test virtual
        clocks behave correctly (a fast virtual clock that says many
        ticks fit inside a window will simply report a high achieved
        rate and skip the warning).
        """
        self._window_ticks += 1
        now = self._clock()
        elapsed = now - self._window_start
        if elapsed < self._window_s:
            return

        achieved_hz = self._window_ticks / elapsed if elapsed > 0.0 else 0.0
        if (
            achieved_hz < self._warn_threshold_hz
            and (now - self._last_warn_at) >= self._warn_throttle_s
        ):
            print(
                f"rollio-device-agx-nero: {self._label} control loop "
                f"running at {achieved_hz:.1f} Hz, below the "
                f"{self._target_hz:.0f} Hz target (warn threshold "
                f"{self._warn_threshold_hz:.1f} Hz). Check CAN bus "
                f"contention, CPU load, or `step()` cost.",
                file=sys.stderr,
            )
            self._last_warn_at = now
        self._window_start = now
        self._window_ticks = 0


__all__ = ["RateMonitor"]
