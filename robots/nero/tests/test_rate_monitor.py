"""Tests for `rollio_device_nero.runtime.rate_monitor.RateMonitor`."""

from __future__ import annotations

import pytest
from rollio_device_nero.runtime.rate_monitor import RateMonitor


def _make_clock() -> tuple[list[float], callable]:
    """Mutable [t] list + closure that reads it. Lets tests advance time."""
    t = [0.0]

    def clock() -> float:
        return t[0]

    return t, clock


def _capture_stderr(capsys: pytest.CaptureFixture[str]) -> str:
    return capsys.readouterr().err


def test_no_warning_when_achieved_rate_meets_target(
    capsys: pytest.CaptureFixture[str],
) -> None:
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="arm",
        clock=clock,
        window_s=1.0,
        warn_throttle_s=10.0,
    )
    # Drive a full window with exactly 250 ticks across 1 s of virtual
    # time -- bang on the target.
    for _ in range(250):
        t[0] += 1.0 / 250.0
        mon.record_tick()
    assert _capture_stderr(capsys) == ""


def test_warns_when_achieved_rate_drops_below_threshold(
    capsys: pytest.CaptureFixture[str],
) -> None:
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="arm",
        clock=clock,
        window_s=1.0,
        warn_throttle_s=10.0,
    )
    # 200 ticks in 1 s -> 200 Hz, well below 250 * 0.95 = 237.5 Hz.
    for _ in range(200):
        t[0] += 1.0 / 200.0
        mon.record_tick()
    err = _capture_stderr(capsys)
    assert "running at 200.0 Hz" in err, err
    assert "250 Hz target" in err
    assert "warn threshold 237.5 Hz" in err


def test_warning_is_throttled_to_one_per_throttle_window(
    capsys: pytest.CaptureFixture[str],
) -> None:
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="arm",
        clock=clock,
        window_s=1.0,
        warn_throttle_s=10.0,
    )
    # Three back-to-back slow windows: only the first should warn,
    # the next two are inside the 10 s throttle window.
    for _ in range(3):
        for _ in range(200):
            t[0] += 1.0 / 200.0
            mon.record_tick()
    err = _capture_stderr(capsys)
    assert err.count("control loop running at") == 1, err


def test_warning_re_arms_after_throttle_window_elapses(
    capsys: pytest.CaptureFixture[str],
) -> None:
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="arm",
        clock=clock,
        window_s=1.0,
        warn_throttle_s=10.0,
    )
    # Slow window 1 -> warn.
    for _ in range(200):
        t[0] += 1.0 / 200.0
        mon.record_tick()
    # Jump 12 s forward (well past the throttle) but record no ticks
    # so the next window's slowness re-arms the warning.
    t[0] += 12.0
    # Slow window 2 -> should warn again.
    for _ in range(200):
        t[0] += 1.0 / 200.0
        mon.record_tick()
    err = _capture_stderr(capsys)
    assert err.count("control loop running at") == 2, err


def test_label_appears_in_warning_text(
    capsys: pytest.CaptureFixture[str],
) -> None:
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="gripper",
        clock=clock,
        window_s=1.0,
        warn_throttle_s=10.0,
    )
    for _ in range(200):
        t[0] += 1.0 / 200.0
        mon.record_tick()
    err = _capture_stderr(capsys)
    assert "gripper control loop" in err


def test_does_not_warn_until_a_full_window_has_elapsed(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Even a slow burst of ticks must wait for the full window before
    we have enough samples to compute a meaningful achieved rate."""
    t, clock = _make_clock()
    mon = RateMonitor(
        target_hz=250.0,
        min_ratio=0.95,
        label="arm",
        clock=clock,
        window_s=5.0,
        warn_throttle_s=10.0,
    )
    # Half a window of slow ticks -- not enough elapsed time yet.
    for _ in range(100):
        t[0] += 1.0 / 200.0
        mon.record_tick()
    assert _capture_stderr(capsys) == ""


def test_invalid_target_hz_rejected() -> None:
    _, clock = _make_clock()
    with pytest.raises(ValueError):
        RateMonitor(target_hz=0.0, min_ratio=0.95, label="x", clock=clock)
    with pytest.raises(ValueError):
        RateMonitor(target_hz=-1.0, min_ratio=0.95, label="x", clock=clock)


def test_invalid_min_ratio_rejected() -> None:
    _, clock = _make_clock()
    with pytest.raises(ValueError):
        RateMonitor(target_hz=250.0, min_ratio=0.0, label="x", clock=clock)
    with pytest.raises(ValueError):
        RateMonitor(target_hz=250.0, min_ratio=1.5, label="x", clock=clock)
