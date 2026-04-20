use clap::{Args, Parser, Subcommand};
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::CONTROL_EVENTS_SERVICE;
use rollio_types::config::{
    MappingStrategy, RobotCommandKind, RobotStateKind, TeleopRuntimeConfigV2,
};
use rollio_types::messages::{
    ControlEvent, JointMitCommand15, JointVector15, ParallelMitCommand2, ParallelVector2, Pose7,
};
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Initial-syncing ramp constants (intentionally not exposed via config).
//
// These values are safety-critical: they bound how aggressively the router
// closes the gap between the leader and the follower at teleop startup. We
// keep them as compile-time constants in the router crate so that operators
// cannot accidentally widen them through a config tweak. If you really need
// to tune them, adjust the constants here and rebuild.
// ---------------------------------------------------------------------------

/// Maximum per-cycle joint step (rad) while the joint-mapping ramp is
/// active. Originally 0.005 rad/tick; bumped 5x to 0.025 rad/tick so the
/// position-error torque (`KP * step`) is large enough to overcome the
/// 40% un-compensated gravity term on the airbot-play's joints 1-3
/// (see `gravity_coefficients` in airbot-play-rust). At a 250 Hz router
/// loop this caps startup joint speed at ~6.25 rad/s (~358 deg/s).
const SYNC_MAX_STEP_RAD: f64 = 0.025;
/// Per-joint distance under which the joint-mapping ramp is considered
/// complete and pass-through forwarding takes over. Sized at ~10x the
/// per-tick step so the ramp has comfortable headroom to terminate even
/// if the operator never aligns the two arms perfectly.
const SYNC_COMPLETE_THRESHOLD_RAD: f64 = 0.25;
/// Maximum per-cycle translational step (metres) while the cartesian ramp
/// is active. The follower's published EE position is moved at most this
/// far toward the leader each tick. Originally 0.001 m/tick; bumped 5x
/// to 0.005 m/tick so the IK-projected joint deltas were large enough
/// for the position loop (`KP * step`) to overcome the airbot-play's
/// 40% un-compensated gravity term on joints 1-3. Then halved back to
/// 0.0025 m/tick to soften startup motion now that gravity tracking is
/// reliable enough at the lower step. At a 250 Hz router loop this
/// caps startup translational speed at ~0.625 m/s.
const SYNC_MAX_STEP_M: f64 = 0.0005;
/// Maximum per-cycle rotational step (rad) while the cartesian ramp is
/// active. The follower's published EE orientation is slerped toward the
/// leader by at most this angle per tick. Halved from 0.01 to 0.005
/// rad/tick to match the slower translational cap; at a 250 Hz router
/// loop this caps startup angular speed at ~1.25 rad/s (~71 deg/s),
/// which is still fast enough to keep up with normal operator hand
/// rotation but noticeably gentler at engagement.
const SYNC_MAX_STEP_ROT_RAD: f64 = 0.001;
/// Translational error (metres) under which the cartesian ramp is
/// considered complete (in conjunction with the rotational threshold).
/// Sized at ~5x the per-tick step so the ramp has comfortable headroom
/// to terminate.
const SYNC_COMPLETE_THRESHOLD_M: f64 = 0.025;
/// Rotational error (rad) under which the cartesian ramp is considered
/// complete (in conjunction with the translational threshold). Sized at
/// ~10x the per-tick rotational step (matching the joint-mapping
/// headroom) so that pass-through can engage once the follower is
/// approximately aligned (~5.7 deg) without requiring the operator to
/// hold the leader within a fraction of a degree of the follower.
const SYNC_COMPLETE_THRESHOLD_ROT_RAD: f64 = 0.1;
/// Wall-clock window of *uninterrupted* leader/follower closeness
/// required before the cartesian ramp declares sync complete and the
/// router drops into pure pass-through. Strict reset: any single tick
/// where the live (leader, follower) gap exceeds either completion
/// threshold zeroes the counter again. Wall-clock instead of tick
/// count keeps this robust to leader publish-rate variation. 1 s is
/// long enough to ride through a transient near-miss but short enough
/// that an operator who held still for "a beat" sees teleop engage
/// promptly.
const SYNC_HOLD_DURATION: Duration = Duration::from_secs(1);

type ControlSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;

#[derive(Debug, Error)]
pub enum TeleopRouterError {
    #[error("leader state only exposes {available} values, required source index {requested}")]
    LeaderValueOutOfRange { requested: usize, available: usize },
    #[error("cartesian forwarding requires pose payloads on both sides")]
    InvalidCartesianRoute,
}

#[derive(Parser, Debug)]
#[command(name = "rollio-teleop-router")]
#[command(about = "Leader-to-follower teleop command forwarding")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,
}

enum LeaderStateSubscriber {
    JointVector15(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    ParallelVector2(iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelVector2, ()>),
    Pose7(iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>),
}

enum FollowerCommandPublisher {
    JointVector15(iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>),
    JointMitCommand15(iceoryx2::port::publisher::Publisher<ipc::Service, JointMitCommand15, ()>),
    ParallelVector2(iceoryx2::port::publisher::Publisher<ipc::Service, ParallelVector2, ()>),
    ParallelMitCommand2(
        iceoryx2::port::publisher::Publisher<ipc::Service, ParallelMitCommand2, ()>,
    ),
    Pose7(iceoryx2::port::publisher::Publisher<ipc::Service, Pose7, ()>),
}

enum LeaderState {
    Vector { timestamp_ms: u64, values: Vec<f64> },
    Pose(Pose7),
}

/// Two-phase teleop ramp shared between the joint-mapping and cartesian
/// teleop policies:
///
/// 1. **Initial syncing** — every published command is clamped so the
///    follower eases toward the leader without snapping. Concretely:
///    - For the *joint* variant, each joint target is at most
///      [`SYNC_MAX_STEP_RAD`] away from the follower's reported joint
///      position. The phase is considered complete once every joint
///      difference is at or below [`SYNC_COMPLETE_THRESHOLD_RAD`].
///    - For the *cartesian* variant, the EE position target moves at
///      most [`SYNC_MAX_STEP_M`] per cycle and the orientation slerps by
///      at most [`SYNC_MAX_STEP_ROT_RAD`] per cycle. The phase is
///      considered complete once both translational and rotational error
///      drop below [`SYNC_COMPLETE_THRESHOLD_M`] / [`SYNC_COMPLETE_THRESHOLD_ROT_RAD`].
/// 2. **Pass-through** — the leader target is forwarded to the follower
///    untouched, including jumps larger than the completion threshold.
///    The rationale (see user spec) is that smoothing big diffs at this
///    stage would inject lag that the operator can't predict, which is
///    more dangerous than letting the follower's lower-level controller
///    decide how to track the new target.
///
/// Sync is automatically disabled (the router defaults to pure
/// pass-through) when the follower-state subscription is missing or when
/// the configured state/command kinds don't match either ramp.
///
/// While syncing, if no follower-state sample has been received yet, the
/// router holds (skips publishing) and waits for follower feedback. This
/// is intentionally stricter than letting the leader value through
/// unclamped: the whole point of the ramp is to ensure the very first
/// command we publish is close to where the follower already is.
///
/// The ramp parameters (`SYNC_MAX_STEP_*` and `SYNC_COMPLETE_THRESHOLD_*`)
/// are intentionally compile-time constants and are NOT exposed via
/// `TeleopRuntimeConfigV2`. They are safety-critical defaults that bound
/// the worst-case startup motion; any deployed config can therefore be
/// audited for correctness without inspecting per-channel overrides. The
/// `sync_max_step_rad` / `sync_complete_threshold_rad` fields on
/// `TeleopRuntimeConfigV2` remain for backwards-compatible deserialisation
/// only and are deliberately ignored by this module.
enum SyncState {
    Disabled,
    Joint(JointSyncState),
    Cartesian(CartesianSyncState),
}

struct JointSyncState {
    synced: bool,
    max_step: f64,
    complete_threshold: f64,
}

struct CartesianSyncState {
    synced: bool,
    max_step_m: f64,
    max_step_rot_rad: f64,
    /// Single threshold reused for both the per-tick step/pass-through
    /// gate AND the sync-complete exit condition. When the live
    /// (leader, follower) gap is within (`complete_threshold_m`,
    /// `complete_threshold_rot_rad`) we publish the leader directly
    /// (no per-tick clamp); otherwise the published target is the
    /// rate-limited ramp anchor.
    complete_threshold_m: f64,
    complete_threshold_rot_rad: f64,
    /// Internal monotonic ramp anchor. Initialized once from the
    /// follower's first reported EE pose, then advanced toward the
    /// leader by at most (`max_step_m`, `max_step_rot_rad`) per tick
    /// when the leader/follower gap is wider than the completion
    /// thresholds. When the gap closes inside the thresholds the
    /// anchor snaps to the leader and the published target is the raw
    /// leader pose (still hemisphere-aligned).
    ///
    /// In the rate-limited branch the published target is the
    /// anchor, NOT a function of the live follower pose, so FK/IK
    /// noise on the follower side cannot leak back into the published
    /// command and create a closed-loop oscillation. (Joint mapping
    /// uses live follower as anchor without problems because
    /// joint->joint mapping has no FK/IK in the loop to amplify
    /// noise; cartesian does, hence the asymmetry.)
    ramp_pose: Option<Pose7>,
    /// Monotonic timestamp at which the (leader, follower) gap first
    /// fell within the completion thresholds during the *current*
    /// streak of closeness. Reset to `None` on any tick the gap
    /// exceeds either threshold (strict). Sync completes once the
    /// streak has lasted at least [`SYNC_HOLD_DURATION`].
    closeness_started_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncOutcome {
    /// The forwarded command (mutated in-place by `apply`) is safe to
    /// publish to the follower this cycle.
    Publish,
    /// The router does not yet have enough information (typically: no
    /// follower-state sample seen) to safely publish a clamped command.
    /// The caller MUST drop this leader sample without publishing and
    /// without advancing the last-forwarded timestamp, so the next loop
    /// iteration re-evaluates as soon as follower feedback arrives.
    Hold,
}

impl SyncState {
    fn new(config: &TeleopRuntimeConfigV2) -> Self {
        let has_follower_topic = config.follower_state_topic.is_some();
        if !has_follower_topic {
            return SyncState::Disabled;
        }
        match (
            config.mapping,
            config.follower_state_kind,
            config.follower_command_kind,
        ) {
            (
                MappingStrategy::Cartesian,
                Some(RobotStateKind::EndEffectorPose),
                RobotCommandKind::EndPose,
            ) => SyncState::Cartesian(CartesianSyncState {
                synced: false,
                max_step_m: SYNC_MAX_STEP_M,
                max_step_rot_rad: SYNC_MAX_STEP_ROT_RAD,
                complete_threshold_m: SYNC_COMPLETE_THRESHOLD_M,
                complete_threshold_rot_rad: SYNC_COMPLETE_THRESHOLD_ROT_RAD,
                ramp_pose: None,
                closeness_started_at: None,
            }),
            (
                MappingStrategy::DirectJoint,
                Some(RobotStateKind::JointPosition) | Some(RobotStateKind::ParallelPosition),
                RobotCommandKind::JointPosition
                | RobotCommandKind::JointMit
                | RobotCommandKind::ParallelPosition
                | RobotCommandKind::ParallelMit,
            ) => SyncState::Joint(JointSyncState {
                synced: false,
                max_step: SYNC_MAX_STEP_RAD,
                complete_threshold: SYNC_COMPLETE_THRESHOLD_RAD,
            }),
            _ => SyncState::Disabled,
        }
    }

    fn enabled(&self) -> bool {
        match self {
            SyncState::Disabled => false,
            SyncState::Joint(s) => !s.synced,
            SyncState::Cartesian(s) => !s.synced,
        }
    }

    /// Returns a short label describing the ramp, used in the startup log
    /// so operators can see at a glance which mode the router booted in.
    fn mode_label(&self) -> &'static str {
        match self {
            SyncState::Disabled => "pass-through",
            SyncState::Joint(_) => "initial-ramp (joint)",
            SyncState::Cartesian(_) => "initial-ramp (cartesian)",
        }
    }

    /// Clamp `command` toward the follower's current state and tell the
    /// caller whether to publish this cycle. Must only be invoked while
    /// `self.enabled()` is true.
    fn apply(
        &mut self,
        command: &mut ForwardedCommand,
        follower: Option<&LeaderState>,
    ) -> SyncOutcome {
        match self {
            SyncState::Disabled => SyncOutcome::Publish,
            SyncState::Joint(state) => state.apply(command, follower),
            SyncState::Cartesian(state) => state.apply(command, follower),
        }
    }
}

impl JointSyncState {
    fn apply(
        &mut self,
        command: &mut ForwardedCommand,
        follower: Option<&LeaderState>,
    ) -> SyncOutcome {
        let Some(follower_values) = follower_position_slice(follower) else {
            // We refuse to publish until the follower has reported its
            // current pose at least once. Holding here matches the
            // safety stance of the cartesian ramp and prevents an
            // unbounded first command from being forwarded if the
            // follower-state subscriber never fires.
            return SyncOutcome::Hold;
        };
        let Some(target) = command_joint_target_mut(command) else {
            // Should be unreachable thanks to the kind check in
            // `SyncState::new`, but bail safely if a future code path
            // builds a non-joint command on the joint ramp.
            return SyncOutcome::Publish;
        };
        let len = target.len().min(follower_values.len());
        let mut max_diff = 0.0f64;
        for i in 0..len {
            let diff = target[i] - follower_values[i];
            max_diff = max_diff.max(diff.abs());
            let clamped = diff.clamp(-self.max_step, self.max_step);
            target[i] = follower_values[i] + clamped;
        }
        if max_diff <= self.complete_threshold {
            self.synced = true;
            eprintln!(
                "rollio-teleop-router: initial sync complete (joint max diff {:.4} rad <= threshold {:.4} rad)",
                max_diff, self.complete_threshold
            );
        }
        SyncOutcome::Publish
    }
}

impl CartesianSyncState {
    fn apply(
        &mut self,
        command: &mut ForwardedCommand,
        follower: Option<&LeaderState>,
    ) -> SyncOutcome {
        self.apply_with_clock(command, follower, std::time::Instant::now())
    }

    /// Internal entry point that takes the "now" instant explicitly so
    /// unit tests can drive the sustained-closeness window deterministically.
    fn apply_with_clock(
        &mut self,
        command: &mut ForwardedCommand,
        follower: Option<&LeaderState>,
        now: std::time::Instant,
    ) -> SyncOutcome {
        let Some(target) = command_pose_mut(command) else {
            return SyncOutcome::Publish;
        };
        let leader_p = pose_position(target);
        let leader_q_raw = pose_quat(target);

        // We need a fresh follower sample to make any decision: the
        // step branch needs the live (leader, follower) gap to size
        // the step direction, and the pass-through branch's
        // sustained-closeness exit condition is also follower-based.
        // Holding (skipping publication) is safer than publishing the
        // raw leader, which could be a large jump from the follower.
        let Some(follower_pose) = follower_pose(follower) else {
            return SyncOutcome::Hold;
        };
        let follower_p = pose_position(&follower_pose);
        let follower_q_raw = pose_quat(&follower_pose);

        // Initialize the internal ramp anchor on the first follower
        // sample we see, so the rate-limited branch always has a
        // sensible starting pose near where the follower actually is.
        if self.ramp_pose.is_none() {
            self.ramp_pose = Some(follower_pose);
        }
        let ramp = self.ramp_pose.as_mut().expect("ramp_pose set above");
        let anchor_p = pose_position(ramp);
        let anchor_q = pose_quat(ramp);

        // Hemisphere-align the leader's and follower's quats against
        // the ramp anchor so all subsequent slerp / angle computations
        // pick the short path consistent with the ramp's progression.
        let leader_q = quat_align_shortest(&leader_q_raw, &anchor_q);
        let follower_q = quat_align_shortest(&follower_q_raw, &anchor_q);

        // Single completion gate: the gap between the LIVE leader and
        // LIVE follower. This is the only place the live follower
        // pose feeds the decision (it is *not* used for the published
        // target in the rate-limited branch -- see anchor docstring).
        let trans_gap = vec3_distance(&leader_p, &follower_p);
        let rot_gap = quat_angle_between(&leader_q, &follower_q);
        let within_thresholds =
            trans_gap <= self.complete_threshold_m && rot_gap <= self.complete_threshold_rot_rad;

        if within_thresholds {
            // Pass-through branch: publish the leader's pose directly,
            // bypassing the per-tick clamp. The bounded translational
            // / rotational gap (<= completion thresholds) caps the
            // worst-case single-tick jump at the threshold size, so
            // there is no need to slew. Snap the anchor to the leader
            // so a future drop back into the rate-limited branch
            // resumes from the right place.
            write_pose(ramp, &leader_p, &leader_q);
            write_pose(target, &leader_p, &leader_q);

            // Sustained-closeness counter: starts on the first close
            // tick of a streak; declares sync complete once the
            // streak has been uninterrupted for SYNC_HOLD_DURATION.
            let started_at = self.closeness_started_at.get_or_insert(now);
            if now.duration_since(*started_at) >= SYNC_HOLD_DURATION {
                self.synced = true;
                eprintln!(
                    "rollio-teleop-router: initial sync complete (cartesian \
                     leader/follower gap {:.4} m, {:.4} rad held within \
                     thresholds {:.4} m, {:.4} rad for {} ms)",
                    trans_gap,
                    rot_gap,
                    self.complete_threshold_m,
                    self.complete_threshold_rot_rad,
                    SYNC_HOLD_DURATION.as_millis(),
                );
            }
        } else {
            // Strict reset: any single tick where the gap exceeds
            // either threshold restarts the closeness counter.
            self.closeness_started_at = None;

            // Rate-limited branch: advance the anchor toward the
            // leader by at most (max_step_m, max_step_rot_rad) and
            // publish that. Anchor stays decoupled from the live
            // follower so FK/IK noise cannot leak into the published
            // command.
            let mut next_p = leader_p;
            clamp_translation(&mut next_p, &anchor_p, self.max_step_m);
            let mut next_q = leader_q;
            clamp_rotation(&mut next_q, &anchor_q, self.max_step_rot_rad);
            write_pose(ramp, &next_p, &next_q);
            write_pose(target, &next_p, &next_q);
        }

        SyncOutcome::Publish
    }
}

fn vec3_distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn follower_position_slice(state: Option<&LeaderState>) -> Option<&[f64]> {
    match state? {
        LeaderState::Vector { values, .. } => Some(values.as_slice()),
        LeaderState::Pose(_) => None,
    }
}

fn follower_pose(state: Option<&LeaderState>) -> Option<Pose7> {
    match state? {
        LeaderState::Pose(pose) => Some(*pose),
        LeaderState::Vector { .. } => None,
    }
}

/// Mutable view into the joint-target portion of a forwarded command.
/// Returns `None` for pose commands; the cartesian ramp uses
/// `command_pose_mut` instead.
fn command_joint_target_mut(command: &mut ForwardedCommand) -> Option<&mut [f64]> {
    match command {
        ForwardedCommand::JointPosition(payload) => {
            let len = payload.len as usize;
            Some(&mut payload.values[..len])
        }
        ForwardedCommand::JointMit(payload) => {
            let len = payload.len as usize;
            Some(&mut payload.position[..len])
        }
        ForwardedCommand::ParallelPosition(payload) => {
            let len = payload.len as usize;
            Some(&mut payload.values[..len])
        }
        ForwardedCommand::ParallelMit(payload) => {
            let len = payload.len as usize;
            Some(&mut payload.position[..len])
        }
        ForwardedCommand::EndPose(_) => None,
    }
}

/// Mutable view into the pose payload of a forwarded command. Returns
/// `None` for joint-space commands; the joint ramp uses
/// `command_joint_target_mut` instead.
fn command_pose_mut(command: &mut ForwardedCommand) -> Option<&mut Pose7> {
    match command {
        ForwardedCommand::EndPose(pose) => Some(pose),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Cartesian helpers
//
// `Pose7.values` layout is `[x, y, z, qx, qy, qz, qw]` (w-last quaternion
// convention, see `robots/airbot_play_rust/src/bin/device.rs`). Helpers
// below operate on plain `[f64; N]` arrays so they can be unit-tested
// without dragging in a quaternion library; if we ever pull in `nalgebra`
// or similar, the implementations can be swapped without changing call
// sites.
// ---------------------------------------------------------------------------

fn pose_position(p: &Pose7) -> [f64; 3] {
    [p.values[0], p.values[1], p.values[2]]
}

fn pose_quat(p: &Pose7) -> [f64; 4] {
    [p.values[3], p.values[4], p.values[5], p.values[6]]
}

fn write_pose(p: &mut Pose7, position: &[f64; 3], quat: &[f64; 4]) {
    p.values[0] = position[0];
    p.values[1] = position[1];
    p.values[2] = position[2];
    p.values[3] = quat[0];
    p.values[4] = quat[1];
    p.values[5] = quat[2];
    p.values[6] = quat[3];
}

fn quat_dot(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// Flip the sign of `target` if it lies on the opposite hemisphere from
/// `reference`, so subsequent slerp/error computations follow the short
/// path. Quaternions `q` and `-q` represent the same rotation.
fn quat_align_shortest(target: &[f64; 4], reference: &[f64; 4]) -> [f64; 4] {
    if quat_dot(target, reference) < 0.0 {
        [-target[0], -target[1], -target[2], -target[3]]
    } else {
        *target
    }
}

/// Angle (rad) between two unit quaternions, assuming they are already
/// aligned to the same hemisphere via `quat_align_shortest`.
fn quat_angle_between(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    // `dot` is `cos(theta/2)` for unit quaternions on the same
    // hemisphere; clamp to guard against floating-point drift outside
    // the legal `[-1, 1]` domain of `acos`.
    let cos_half = quat_dot(a, b).clamp(-1.0, 1.0);
    2.0 * cos_half.abs().min(1.0).acos()
}

/// Standard spherical linear interpolation. `t` is clamped to `[0, 1]`.
/// `from` and `to` are expected to be unit quaternions on the same
/// hemisphere (see `quat_align_shortest`).
fn quat_slerp(from: &[f64; 4], to: &[f64; 4], t: f64) -> [f64; 4] {
    let t = t.clamp(0.0, 1.0);
    let dot = quat_dot(from, to).clamp(-1.0, 1.0);
    // For nearly-parallel quaternions, fall back to normalised lerp to
    // avoid dividing by a near-zero `sin(theta)`.
    if dot.abs() > 0.9995 {
        let mut out = [
            from[0] + t * (to[0] - from[0]),
            from[1] + t * (to[1] - from[1]),
            from[2] + t * (to[2] - from[2]),
            from[3] + t * (to[3] - from[3]),
        ];
        normalise_quat(&mut out);
        return out;
    }
    let theta = dot.acos();
    let sin_theta = theta.sin();
    let s_from = ((1.0 - t) * theta).sin() / sin_theta;
    let s_to = (t * theta).sin() / sin_theta;
    let mut out = [
        s_from * from[0] + s_to * to[0],
        s_from * from[1] + s_to * to[1],
        s_from * from[2] + s_to * to[2],
        s_from * from[3] + s_to * to[3],
    ];
    normalise_quat(&mut out);
    out
}

fn normalise_quat(q: &mut [f64; 4]) {
    let norm_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if norm_sq > 0.0 {
        let inv = 1.0 / norm_sq.sqrt();
        q[0] *= inv;
        q[1] *= inv;
        q[2] *= inv;
        q[3] *= inv;
    }
}

/// Move `target` so it is at most `max_step` away (Euclidean norm in
/// metres) from `anchor`. Returns the original distance between the two
/// before clamping, so the caller can decide whether the ramp is
/// complete.
fn clamp_translation(target: &mut [f64; 3], anchor: &[f64; 3], max_step: f64) -> f64 {
    let dx = target[0] - anchor[0];
    let dy = target[1] - anchor[1];
    let dz = target[2] - anchor[2];
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    if dist > max_step {
        let scale = max_step / dist;
        target[0] = anchor[0] + dx * scale;
        target[1] = anchor[1] + dy * scale;
        target[2] = anchor[2] + dz * scale;
    }
    dist
}

/// Slerp `target_q` toward `anchor_q`'s neighbourhood by at most
/// `max_step` radians. Returns the original angular distance between the
/// two (before clamping). Both quaternions must already be aligned to
/// the same hemisphere via `quat_align_shortest`.
fn clamp_rotation(target_q: &mut [f64; 4], anchor_q: &[f64; 4], max_step: f64) -> f64 {
    let angle = quat_angle_between(anchor_q, target_q);
    if angle > max_step && angle > f64::EPSILON {
        let t = max_step / angle;
        // Slerp from the anchor toward the leader's target by the
        // fraction that corresponds to `max_step` radians.
        let stepped = quat_slerp(anchor_q, target_q, t);
        target_q[0] = stepped[0];
        target_q[1] = stepped[1];
        target_q[2] = stepped[2];
        target_q[3] = stepped[3];
    }
    angle
}

pub fn run_cli() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => {
            let config = load_runtime_config(&args)?;
            run_router(config)
        }
    }
}

fn load_runtime_config(args: &RunArgs) -> Result<TeleopRuntimeConfigV2, Box<dyn Error>> {
    match (&args.config, &args.config_inline) {
        (Some(path), None) => Ok(TeleopRuntimeConfigV2::from_file(path)?),
        (None, Some(inline)) => Ok(inline.parse::<TeleopRuntimeConfigV2>()?),
        (None, None) => Err("teleop router requires --config or --config-inline".into()),
        (Some(_), Some(_)) => Err("teleop router config flags are mutually exclusive".into()),
    }
}

pub fn run_router(config: TeleopRuntimeConfigV2) -> Result<(), Box<dyn Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let leader_state_subscriber =
        create_state_subscriber(&node, &config.leader_state_topic, config.leader_state_kind)?;
    let follower_state_subscriber = match (
        config.follower_state_kind,
        config.follower_state_topic.as_deref(),
    ) {
        (Some(kind), Some(topic)) => Some(create_state_subscriber(&node, topic, kind)?),
        _ => None,
    };
    let follower_command_publisher = create_command_publisher(
        &node,
        &config.follower_command_topic,
        config.follower_command_kind,
    )?;
    let control_subscriber = create_control_subscriber(&node)?;
    let mut last_forwarded_timestamp_ms = None;
    let mut sync_state = SyncState::new(&config);
    let mut follower_state: Option<LeaderState> = None;

    eprintln!(
        "rollio-teleop-router: {} forwarding {} -> {} with {:?} (sync mode: {})",
        config.process_id,
        config.leader_channel_id,
        config.follower_channel_id,
        config.mapping,
        sync_state.mode_label(),
    );

    loop {
        if drain_control_events(&control_subscriber)? {
            break;
        }
        // Always drain the follower state subscriber so the syncing phase
        // sees the freshest position. If the follower hasn't booted yet the
        // drain is a no-op and `follower_state` simply stays at `None`.
        if let Some(subscriber) = follower_state_subscriber.as_ref() {
            if let Some(state) = drain_latest_state(subscriber)? {
                follower_state = Some(state);
            }
        }
        if let Some(state) = drain_latest_state(&leader_state_subscriber)? {
            let timestamp_ms = state_timestamp_ms(&state);
            if last_forwarded_timestamp_ms == Some(timestamp_ms) {
                continue;
            }
            let mapped = map_leader_state(&config, &state)?;
            if let Some(mut forwarded) = mapped {
                let outcome = if sync_state.enabled() {
                    sync_state.apply(&mut forwarded, follower_state.as_ref())
                } else {
                    SyncOutcome::Publish
                };
                match outcome {
                    SyncOutcome::Publish => {
                        publish_command(&follower_command_publisher, forwarded)?;
                        last_forwarded_timestamp_ms = Some(timestamp_ms);
                    }
                    SyncOutcome::Hold => {
                        // Deliberately do NOT advance
                        // `last_forwarded_timestamp_ms`: we want the
                        // next loop iteration to re-evaluate the same
                        // leader sample once follower feedback lands,
                        // so the very first published command is
                        // close to the follower's current pose.
                    }
                }
                continue;
            }
        }

        match node.wait(Duration::from_millis(1)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

    eprintln!(
        "rollio-teleop-router: {} shutdown complete",
        config.process_id
    );
    Ok(())
}

fn create_state_subscriber(
    node: &Node<ipc::Service>,
    topic: &str,
    kind: RobotStateKind,
) -> Result<LeaderStateSubscriber, Box<dyn Error>> {
    let service_name: ServiceName = topic.try_into()?;
    Ok(match kind {
        RobotStateKind::EndEffectorPose => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<Pose7>()
                .open_or_create()?;
            LeaderStateSubscriber::Pose7(service.subscriber_builder().create()?)
        }
        RobotStateKind::ParallelPosition
        | RobotStateKind::ParallelVelocity
        | RobotStateKind::ParallelEffort => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<ParallelVector2>()
                .open_or_create()?;
            LeaderStateSubscriber::ParallelVector2(service.subscriber_builder().create()?)
        }
        _ => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<JointVector15>()
                .open_or_create()?;
            LeaderStateSubscriber::JointVector15(service.subscriber_builder().create()?)
        }
    })
}

fn create_command_publisher(
    node: &Node<ipc::Service>,
    topic: &str,
    kind: RobotCommandKind,
) -> Result<FollowerCommandPublisher, Box<dyn Error>> {
    let service_name: ServiceName = topic.try_into()?;
    Ok(match kind {
        RobotCommandKind::JointPosition => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<JointVector15>()
                .open_or_create()?;
            FollowerCommandPublisher::JointVector15(service.publisher_builder().create()?)
        }
        RobotCommandKind::JointMit => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<JointMitCommand15>()
                .open_or_create()?;
            FollowerCommandPublisher::JointMitCommand15(service.publisher_builder().create()?)
        }
        RobotCommandKind::ParallelPosition => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<ParallelVector2>()
                .open_or_create()?;
            FollowerCommandPublisher::ParallelVector2(service.publisher_builder().create()?)
        }
        RobotCommandKind::ParallelMit => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<ParallelMitCommand2>()
                .open_or_create()?;
            FollowerCommandPublisher::ParallelMitCommand2(service.publisher_builder().create()?)
        }
        RobotCommandKind::EndPose => {
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<Pose7>()
                .open_or_create()?;
            FollowerCommandPublisher::Pose7(service.publisher_builder().create()?)
        }
    })
}

fn create_control_subscriber(
    node: &Node<ipc::Service>,
) -> Result<ControlSubscriber, Box<dyn Error>> {
    let service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    Ok(service.subscriber_builder().create()?)
}

fn drain_control_events(subscriber: &ControlSubscriber) -> Result<bool, Box<dyn Error>> {
    loop {
        match subscriber.receive()? {
            Some(sample) => {
                if matches!(*sample.payload(), ControlEvent::Shutdown) {
                    return Ok(true);
                }
            }
            None => return Ok(false),
        }
    }
}

fn drain_latest_state(
    subscriber: &LeaderStateSubscriber,
) -> Result<Option<LeaderState>, Box<dyn Error>> {
    let mut latest = None;
    match subscriber {
        LeaderStateSubscriber::JointVector15(subscriber) => loop {
            let Some(sample) = subscriber.receive()? else {
                return Ok(latest);
            };
            let payload = *sample.payload();
            latest = Some(LeaderState::Vector {
                timestamp_ms: payload.timestamp_ms,
                values: payload.values[..payload.len as usize].to_vec(),
            });
        },
        LeaderStateSubscriber::ParallelVector2(subscriber) => loop {
            let Some(sample) = subscriber.receive()? else {
                return Ok(latest);
            };
            let payload = *sample.payload();
            latest = Some(LeaderState::Vector {
                timestamp_ms: payload.timestamp_ms,
                values: payload.values[..payload.len as usize].to_vec(),
            });
        },
        LeaderStateSubscriber::Pose7(subscriber) => loop {
            let Some(sample) = subscriber.receive()? else {
                return Ok(latest);
            };
            latest = Some(LeaderState::Pose(*sample.payload()));
        },
    }
}

fn state_timestamp_ms(state: &LeaderState) -> u64 {
    match state {
        LeaderState::Vector { timestamp_ms, .. } => *timestamp_ms,
        LeaderState::Pose(payload) => payload.timestamp_ms,
    }
}

#[allow(clippy::large_enum_variant)]
enum ForwardedCommand {
    JointPosition(JointVector15),
    JointMit(JointMitCommand15),
    ParallelPosition(ParallelVector2),
    ParallelMit(ParallelMitCommand2),
    EndPose(Pose7),
}

fn map_leader_state(
    config: &TeleopRuntimeConfigV2,
    state: &LeaderState,
) -> Result<Option<ForwardedCommand>, TeleopRouterError> {
    match config.mapping {
        MappingStrategy::Cartesian => match (state, config.follower_command_kind) {
            (LeaderState::Pose(pose), RobotCommandKind::EndPose) => {
                Ok(Some(ForwardedCommand::EndPose(*pose)))
            }
            _ => Err(TeleopRouterError::InvalidCartesianRoute),
        },
        MappingStrategy::DirectJoint => {
            // Per the teleop-policy redesign, DirectJoint owns
            // joint-space teleop only. Parallel-grip routing moved to
            // its own arm. Reject any parallel command kind here so a
            // schema mismatch surfaces early instead of silently
            // dispatching the wrong payload type.
            let LeaderState::Vector {
                timestamp_ms,
                values,
            } = state
            else {
                return Err(TeleopRouterError::InvalidCartesianRoute);
            };
            let mapped =
                apply_joint_mapping(values, &config.joint_index_map, &config.joint_scales)?;
            Ok(Some(match config.follower_command_kind {
                RobotCommandKind::JointPosition => ForwardedCommand::JointPosition(
                    JointVector15::from_slice(*timestamp_ms, &mapped),
                ),
                RobotCommandKind::JointMit => ForwardedCommand::JointMit(joint_mit_command(
                    *timestamp_ms,
                    &mapped,
                    &config.command_defaults.joint_mit_kp,
                    &config.command_defaults.joint_mit_kd,
                )),
                RobotCommandKind::ParallelPosition
                | RobotCommandKind::ParallelMit
                | RobotCommandKind::EndPose => {
                    return Err(TeleopRouterError::InvalidCartesianRoute);
                }
            }))
        }
        MappingStrategy::Parallel => {
            // Parallel teleop: dof=1 leader publishes a single-element
            // vector; the controller scales it by `joint_scales[0]`
            // (the operator-supplied ratio) and forwards as
            // ParallelMit when the follower advertised it (kp/kd from
            // command_defaults), otherwise as ParallelPosition.
            let LeaderState::Vector {
                timestamp_ms,
                values,
            } = state
            else {
                return Err(TeleopRouterError::InvalidCartesianRoute);
            };
            let ratio = config.joint_scales.first().copied().unwrap_or(1.0);
            let mapped: Vec<f64> = values.iter().map(|value| value * ratio).collect();
            Ok(Some(match config.follower_command_kind {
                RobotCommandKind::ParallelPosition => ForwardedCommand::ParallelPosition(
                    ParallelVector2::from_slice(*timestamp_ms, &mapped),
                ),
                RobotCommandKind::ParallelMit => {
                    ForwardedCommand::ParallelMit(parallel_mit_command(
                        *timestamp_ms,
                        &mapped,
                        &config.command_defaults.parallel_mit_kp,
                        &config.command_defaults.parallel_mit_kd,
                    ))
                }
                RobotCommandKind::JointPosition
                | RobotCommandKind::JointMit
                | RobotCommandKind::EndPose => {
                    return Err(TeleopRouterError::InvalidCartesianRoute);
                }
            }))
        }
    }
}

fn apply_joint_mapping(
    values: &[f64],
    joint_index_map: &[u32],
    joint_scales: &[f64],
) -> Result<Vec<f64>, TeleopRouterError> {
    let output_len = if !joint_index_map.is_empty() {
        joint_index_map.len()
    } else if !joint_scales.is_empty() {
        joint_scales.len()
    } else {
        values.len()
    };
    let mut mapped = Vec::with_capacity(output_len);
    for output_index in 0..output_len {
        let source_index = joint_index_map
            .get(output_index)
            .copied()
            .unwrap_or(output_index as u32) as usize;
        let Some(value) = values.get(source_index).copied() else {
            return Err(TeleopRouterError::LeaderValueOutOfRange {
                requested: source_index,
                available: values.len(),
            });
        };
        let scale = joint_scales.get(output_index).copied().unwrap_or(1.0);
        mapped.push(value * scale);
    }
    Ok(mapped)
}

fn joint_mit_command(
    timestamp_ms: u64,
    values: &[f64],
    kp: &[f64],
    kd: &[f64],
) -> JointMitCommand15 {
    let len = values.len().min(rollio_types::messages::MAX_DOF);
    let mut command = JointMitCommand15 {
        timestamp_ms,
        len: len as u32,
        ..JointMitCommand15::default()
    };
    command.position[..len].copy_from_slice(&values[..len]);
    for index in 0..len {
        command.kp[index] = kp.get(index).copied().unwrap_or(0.0);
        command.kd[index] = kd.get(index).copied().unwrap_or(0.0);
    }
    command
}

fn parallel_mit_command(
    timestamp_ms: u64,
    values: &[f64],
    kp: &[f64],
    kd: &[f64],
) -> ParallelMitCommand2 {
    let len = values.len().min(rollio_types::messages::MAX_PARALLEL);
    let mut command = ParallelMitCommand2 {
        timestamp_ms,
        len: len as u32,
        ..ParallelMitCommand2::default()
    };
    command.position[..len].copy_from_slice(&values[..len]);
    for index in 0..len {
        command.kp[index] = kp.get(index).copied().unwrap_or(0.0);
        command.kd[index] = kd.get(index).copied().unwrap_or(0.0);
    }
    command
}

fn publish_command(
    publisher: &FollowerCommandPublisher,
    command: ForwardedCommand,
) -> Result<(), Box<dyn Error>> {
    match (publisher, command) {
        (
            FollowerCommandPublisher::JointVector15(publisher),
            ForwardedCommand::JointPosition(command),
        ) => {
            publisher.send_copy(command)?;
        }
        (
            FollowerCommandPublisher::JointMitCommand15(publisher),
            ForwardedCommand::JointMit(command),
        ) => {
            publisher.send_copy(command)?;
        }
        (
            FollowerCommandPublisher::ParallelVector2(publisher),
            ForwardedCommand::ParallelPosition(command),
        ) => {
            publisher.send_copy(command)?;
        }
        (
            FollowerCommandPublisher::ParallelMitCommand2(publisher),
            ForwardedCommand::ParallelMit(command),
        ) => {
            publisher.send_copy(command)?;
        }
        (FollowerCommandPublisher::Pose7(publisher), ForwardedCommand::EndPose(command)) => {
            publisher.send_copy(command)?;
        }
        _ => return Err(
            "teleop router produced a command type that does not match the configured publisher"
                .into(),
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollio_types::config::ChannelCommandDefaults;

    fn direct_config() -> TeleopRuntimeConfigV2 {
        TeleopRuntimeConfigV2 {
            process_id: "teleop.test".into(),
            leader_channel_id: "leader/arm".into(),
            follower_channel_id: "follower/arm".into(),
            leader_state_kind: RobotStateKind::JointPosition,
            leader_state_topic: "leader/arm/states/joint_position".into(),
            follower_command_kind: RobotCommandKind::JointPosition,
            follower_command_topic: "follower/arm/commands/joint_position".into(),
            follower_state_kind: None,
            follower_state_topic: None,
            sync_max_step_rad: None,
            sync_complete_threshold_rad: None,
            mapping: MappingStrategy::DirectJoint,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
            command_defaults: ChannelCommandDefaults::default(),
        }
    }

    #[test]
    fn direct_joint_identity_mapping_preserves_positions() {
        let config = direct_config();
        let state = LeaderState::Vector {
            timestamp_ms: 123,
            values: vec![0.1, 0.2, 0.3],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::JointPosition(command)) = command else {
            panic!("expected joint-position command");
        };
        assert_eq!(&command.values[..3], &[0.1, 0.2, 0.3]);
    }

    #[test]
    fn direct_joint_remap_reorders_source_joints() {
        let mut config = direct_config();
        config.joint_index_map = vec![2, 1, 0];
        let state = LeaderState::Vector {
            timestamp_ms: 123,
            values: vec![0.1, 0.2, 0.3],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::JointPosition(command)) = command else {
            panic!("expected joint-position command");
        };
        assert_eq!(&command.values[..3], &[0.3, 0.2, 0.1]);
    }

    #[test]
    fn direct_joint_scaling_is_applied_per_output_joint() {
        let mut config = direct_config();
        config.joint_scales = vec![2.0, 1.0, 0.5];
        let state = LeaderState::Vector {
            timestamp_ms: 123,
            values: vec![0.1, 0.2, 0.6],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::JointPosition(command)) = command else {
            panic!("expected joint-position command");
        };
        assert_eq!(command.values[0], 0.2);
        assert_eq!(command.values[2], 0.3);
    }

    #[test]
    fn direct_joint_mit_uses_default_gains() {
        let mut config = direct_config();
        config.follower_command_kind = RobotCommandKind::JointMit;
        config.command_defaults = ChannelCommandDefaults {
            joint_mit_kp: vec![10.0, 20.0, 30.0],
            joint_mit_kd: vec![1.0, 2.0, 3.0],
            parallel_mit_kp: Vec::new(),
            parallel_mit_kd: Vec::new(),
        };
        let state = LeaderState::Vector {
            timestamp_ms: 123,
            values: vec![0.1, 0.2, 0.3],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::JointMit(command)) = command else {
            panic!("expected joint-mit command");
        };
        assert_eq!(&command.position[..3], &[0.1, 0.2, 0.3]);
        assert_eq!(&command.kp[..3], &[10.0, 20.0, 30.0]);
        assert_eq!(&command.kd[..3], &[1.0, 2.0, 3.0]);
    }

    fn parallel_config(ratio: f64, follower_kind: RobotCommandKind) -> TeleopRuntimeConfigV2 {
        TeleopRuntimeConfigV2 {
            process_id: "teleop.parallel".into(),
            leader_channel_id: "leader/gripper".into(),
            follower_channel_id: "follower/gripper".into(),
            leader_state_kind: RobotStateKind::ParallelPosition,
            leader_state_topic: "leader/gripper/states/parallel_position".into(),
            follower_command_kind: follower_kind,
            follower_command_topic: "follower/gripper/commands/parallel_position".into(),
            follower_state_kind: None,
            follower_state_topic: None,
            sync_max_step_rad: None,
            sync_complete_threshold_rad: None,
            mapping: MappingStrategy::Parallel,
            joint_index_map: Vec::new(),
            joint_scales: vec![ratio],
            command_defaults: ChannelCommandDefaults::default(),
        }
    }

    #[test]
    fn parallel_policy_scales_position_value() {
        let config = parallel_config(2.5, RobotCommandKind::ParallelPosition);
        let state = LeaderState::Vector {
            timestamp_ms: 42,
            values: vec![0.04],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::ParallelPosition(command)) = command else {
            panic!("expected ParallelPosition command");
        };
        // ratio * leader_value = 2.5 * 0.04 = 0.1
        assert!(
            (command.values[0] - 0.1).abs() < 1e-9,
            "expected scaled value ~ 0.1, got {}",
            command.values[0],
        );
    }

    #[test]
    fn parallel_policy_emits_parallel_mit_when_follower_advertises_mit() {
        let mut config = parallel_config(1.0, RobotCommandKind::ParallelMit);
        config.command_defaults = ChannelCommandDefaults {
            joint_mit_kp: Vec::new(),
            joint_mit_kd: Vec::new(),
            parallel_mit_kp: vec![3.0],
            parallel_mit_kd: vec![0.5],
        };
        let state = LeaderState::Vector {
            timestamp_ms: 7,
            values: vec![0.05],
        };
        let command = map_leader_state(&config, &state).expect("mapping should work");
        let Some(ForwardedCommand::ParallelMit(command)) = command else {
            panic!("expected ParallelMit command");
        };
        assert!((command.position[0] - 0.05).abs() < 1e-9);
        assert!((command.kp[0] - 3.0).abs() < 1e-9);
        assert!((command.kd[0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cartesian_mapping_forwards_pose() {
        let mut config = direct_config();
        config.mapping = MappingStrategy::Cartesian;
        config.leader_state_kind = RobotStateKind::EndEffectorPose;
        config.follower_command_kind = RobotCommandKind::EndPose;
        let pose = Pose7 {
            timestamp_ms: 123,
            values: [0.3, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0],
        };
        let command =
            map_leader_state(&config, &LeaderState::Pose(pose)).expect("mapping should work");
        let Some(ForwardedCommand::EndPose(command)) = command else {
            panic!("expected pose command");
        };
        assert_eq!(command.values, pose.values);
    }

    fn sync_config() -> TeleopRuntimeConfigV2 {
        let mut config = direct_config();
        config.follower_state_kind = Some(RobotStateKind::JointPosition);
        config.follower_state_topic = Some("follower/arm/states/joint_position".into());
        config
    }

    fn cartesian_sync_config() -> TeleopRuntimeConfigV2 {
        let mut config = direct_config();
        config.mapping = MappingStrategy::Cartesian;
        config.leader_state_kind = RobotStateKind::EndEffectorPose;
        config.leader_state_topic = "leader/arm/states/end_effector_pose".into();
        config.follower_command_kind = RobotCommandKind::EndPose;
        config.follower_command_topic = "follower/arm/commands/end_pose".into();
        config.follower_state_kind = Some(RobotStateKind::EndEffectorPose);
        config.follower_state_topic = Some("follower/arm/states/end_effector_pose".into());
        config
    }

    #[test]
    fn sync_disabled_when_no_follower_state_configured() {
        let config = direct_config();
        let sync_state = SyncState::new(&config);
        assert!(!sync_state.enabled());
        assert!(matches!(sync_state, SyncState::Disabled));
    }

    #[test]
    fn sync_enabled_when_follower_state_configured() {
        let config = sync_config();
        let sync_state = SyncState::new(&config);
        assert!(sync_state.enabled());
        assert!(matches!(sync_state, SyncState::Joint(_)));
    }

    #[test]
    fn sync_clamps_command_to_max_step_when_far_from_target() {
        let config = sync_config();
        let mut sync_state = SyncState::new(&config);
        let mut command = ForwardedCommand::JointPosition(JointVector15::from_slice(
            123,
            &[0.5, -0.4, 0.3, 0.2, 0.1, 0.0],
        ));
        let follower = LeaderState::Vector {
            timestamp_ms: 100,
            values: vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        };
        let outcome = sync_state.apply(&mut command, Some(&follower));
        assert_eq!(outcome, SyncOutcome::Publish);
        let ForwardedCommand::JointPosition(command) = command else {
            panic!("expected joint-position command");
        };
        // Each joint should be at most SYNC_MAX_STEP_RAD away from the
        // corresponding follower position (i.e. clamped from the
        // leader's larger delta).
        let positions = &command.values[..6];
        for (i, value) in positions.iter().enumerate() {
            assert!(
                value.abs() <= SYNC_MAX_STEP_RAD + f64::EPSILON,
                "joint {} clamped to {}, expected within {} of follower (0.0)",
                i,
                value,
                SYNC_MAX_STEP_RAD,
            );
        }
        // Sync remains active because the leader is still well above the
        // completion threshold.
        assert!(sync_state.enabled());
    }

    #[test]
    fn sync_completes_once_within_threshold() {
        let config = sync_config();
        let mut sync_state = SyncState::new(&config);
        // Use values strictly inside SYNC_COMPLETE_THRESHOLD_RAD (0.05
        // rad) so a single apply finishes the ramp.
        let mut command = ForwardedCommand::JointPosition(JointVector15::from_slice(
            123,
            &[0.04, 0.04, 0.04, 0.04, 0.04, 0.04],
        ));
        let follower = LeaderState::Vector {
            timestamp_ms: 100,
            values: vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        };
        let outcome = sync_state.apply(&mut command, Some(&follower));
        assert_eq!(outcome, SyncOutcome::Publish);
        // Once the difference is <= the threshold the router exits the
        // syncing phase.
        assert!(!sync_state.enabled());
    }

    #[test]
    fn sync_holds_when_no_follower_feedback() {
        // If the follower never reports a state we deliberately refuse
        // to publish (Hold) instead of forwarding the leader's value
        // unclamped. Stays in syncing mode so the next cycle can retry.
        let config = sync_config();
        let mut sync_state = SyncState::new(&config);
        let mut command = ForwardedCommand::JointPosition(JointVector15::from_slice(
            123,
            &[0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
        ));
        let outcome = sync_state.apply(&mut command, None);
        assert_eq!(outcome, SyncOutcome::Hold);
        let ForwardedCommand::JointPosition(command) = command else {
            panic!("expected joint-position command");
        };
        // Command was left untouched so the caller can re-evaluate it
        // next iteration once follower feedback lands.
        assert_eq!(&command.values[..6], &[0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
        assert!(sync_state.enabled());
    }

    #[test]
    fn sync_passes_through_after_completion_even_for_big_jumps() {
        // Reflects the user spec: once sync completes, large diffs are
        // forwarded as-is because rate-limiting at this stage would
        // inject dangerous lag. The actual pass-through is driven by
        // the loop's `enabled()` gate, so this test documents the
        // invariant.
        let config = sync_config();
        let mut sync_state = SyncState::new(&config);
        let SyncState::Joint(ref mut joint) = sync_state else {
            panic!("expected joint sync state");
        };
        joint.synced = true;
        assert!(!sync_state.enabled());
    }

    // -------------------------------------------------------------------
    // Cartesian sync tests
    // -------------------------------------------------------------------

    #[test]
    fn cartesian_sync_disabled_when_no_follower_state_configured() {
        let mut config = cartesian_sync_config();
        config.follower_state_topic = None;
        config.follower_state_kind = None;
        let sync_state = SyncState::new(&config);
        assert!(!sync_state.enabled());
        assert!(matches!(sync_state, SyncState::Disabled));
    }

    #[test]
    fn cartesian_sync_enabled_when_follower_pose_configured() {
        let config = cartesian_sync_config();
        let sync_state = SyncState::new(&config);
        assert!(sync_state.enabled());
        assert!(matches!(sync_state, SyncState::Cartesian(_)));
    }

    #[test]
    fn cartesian_sync_clamps_translation_when_far() {
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        // Leader is 1 m away from the follower along +X. The follower
        // reports the origin with identity orientation.
        let mut command = ForwardedCommand::EndPose(Pose7 {
            timestamp_ms: 123,
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        });
        let follower = LeaderState::Pose(Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        });
        let outcome = sync_state.apply(&mut command, Some(&follower));
        assert_eq!(outcome, SyncOutcome::Publish);
        let ForwardedCommand::EndPose(command) = command else {
            panic!("expected pose command");
        };
        let dx = command.values[0];
        let dy = command.values[1];
        let dz = command.values[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        assert!(
            dist <= SYNC_MAX_STEP_M + 1e-9,
            "translational step {} exceeded SYNC_MAX_STEP_M ({})",
            dist,
            SYNC_MAX_STEP_M,
        );
        // Step direction should still point toward the leader (+X).
        assert!(dx > 0.0);
        // Sync remains active: 1 m >> SYNC_COMPLETE_THRESHOLD_M.
        assert!(sync_state.enabled());
    }

    #[test]
    fn cartesian_sync_clamps_rotation_when_far() {
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        // Leader is +90 deg around Z relative to follower (identity).
        // qz = sin(45 deg), qw = cos(45 deg).
        let half_angle = std::f64::consts::FRAC_PI_4;
        let mut command = ForwardedCommand::EndPose(Pose7 {
            timestamp_ms: 123,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, half_angle.sin(), half_angle.cos()],
        });
        let follower = LeaderState::Pose(Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        });
        let outcome = sync_state.apply(&mut command, Some(&follower));
        assert_eq!(outcome, SyncOutcome::Publish);
        let ForwardedCommand::EndPose(command) = command else {
            panic!("expected pose command");
        };
        let target_q = pose_quat(&command);
        let identity = [0.0, 0.0, 0.0, 1.0];
        let angle = quat_angle_between(&identity, &target_q);
        assert!(
            angle <= SYNC_MAX_STEP_ROT_RAD + 1e-9,
            "rotational step {} exceeded SYNC_MAX_STEP_ROT_RAD ({})",
            angle,
            SYNC_MAX_STEP_ROT_RAD,
        );
        // The step should still rotate around +Z (toward the leader).
        assert!(target_q[2] > 0.0);
        assert!(sync_state.enabled());
    }

    /// Helper: drive `apply_with_clock` directly so we can step the
    /// virtual wall clock past `SYNC_HOLD_DURATION` without sleeping.
    fn cart_apply(
        sync_state: &mut SyncState,
        leader: Pose7,
        follower: Pose7,
        now: std::time::Instant,
    ) -> Pose7 {
        let SyncState::Cartesian(cart) = sync_state else {
            panic!("expected cartesian sync state");
        };
        let mut command = ForwardedCommand::EndPose(leader);
        let outcome = cart.apply_with_clock(&mut command, Some(&LeaderState::Pose(follower)), now);
        assert_eq!(outcome, SyncOutcome::Publish);
        let ForwardedCommand::EndPose(payload) = command else {
            panic!("expected pose command");
        };
        payload
    }

    #[test]
    fn cartesian_sync_publishes_leader_directly_when_within_thresholds() {
        // Within-threshold ticks should bypass the per-tick clamp and
        // forward the leader pose verbatim (the bounded gap caps the
        // worst-case single-tick jump at the threshold size).
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let small_translation = SYNC_COMPLETE_THRESHOLD_M * 0.5;
        let small_half_angle = SYNC_COMPLETE_THRESHOLD_ROT_RAD * 0.25;
        let leader = Pose7 {
            timestamp_ms: 123,
            values: [
                small_translation,
                0.0,
                0.0,
                0.0,
                0.0,
                small_half_angle.sin(),
                small_half_angle.cos(),
            ],
        };
        let follower = Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let now = std::time::Instant::now();
        let published = cart_apply(&mut sync_state, leader, follower, now);
        assert_eq!(
            published.values, leader.values,
            "within-threshold publish should equal the leader exactly"
        );
        // Single tick of closeness is not enough to declare sync
        // complete -- the SYNC_HOLD_DURATION (1 s) window has only
        // just started.
        assert!(sync_state.enabled());
    }

    #[test]
    fn cartesian_sync_completes_only_after_sustained_closeness_window() {
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let small_translation = SYNC_COMPLETE_THRESHOLD_M * 0.5;
        let leader = Pose7 {
            timestamp_ms: 123,
            values: [small_translation, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let follower = Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let t0 = std::time::Instant::now();

        // First close tick starts the streak; sync remains active.
        cart_apply(&mut sync_state, leader, follower, t0);
        assert!(
            sync_state.enabled(),
            "single close tick must not complete sync"
        );

        // Halfway through the window: still not complete.
        cart_apply(
            &mut sync_state,
            leader,
            follower,
            t0 + SYNC_HOLD_DURATION / 2,
        );
        assert!(
            sync_state.enabled(),
            "halfway through hold window: not complete"
        );

        // Past the full SYNC_HOLD_DURATION: sync completes.
        cart_apply(&mut sync_state, leader, follower, t0 + SYNC_HOLD_DURATION);
        assert!(
            !sync_state.enabled(),
            "after SYNC_HOLD_DURATION of uninterrupted closeness, sync must complete"
        );
    }

    #[test]
    fn cartesian_sync_strict_reset_on_transient_breach() {
        // A single tick where the live (leader, follower) gap exceeds
        // either threshold must zero the closeness counter, even if
        // the very next tick is back in range.
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let close_leader = Pose7 {
            timestamp_ms: 123,
            values: [
                SYNC_COMPLETE_THRESHOLD_M * 0.5,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ],
        };
        let far_leader = Pose7 {
            timestamp_ms: 124,
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let follower = Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let t0 = std::time::Instant::now();

        // Close for almost the full window...
        cart_apply(&mut sync_state, close_leader, follower, t0);
        cart_apply(
            &mut sync_state,
            close_leader,
            follower,
            t0 + SYNC_HOLD_DURATION - Duration::from_millis(1),
        );
        // ...then a single far tick resets.
        cart_apply(
            &mut sync_state,
            far_leader,
            follower,
            t0 + SYNC_HOLD_DURATION,
        );
        // ...so even at t0 + 2 * SYNC_HOLD_DURATION sync is NOT complete
        // unless the close streak has restarted and run for the full
        // window again. Right after the breach the counter is zero, so
        // a single close tick at t0 + 2 * SYNC_HOLD_DURATION is the
        // start of a new streak.
        cart_apply(
            &mut sync_state,
            close_leader,
            follower,
            t0 + SYNC_HOLD_DURATION + Duration::from_millis(1),
        );
        assert!(
            sync_state.enabled(),
            "transient breach must reset the closeness counter"
        );
    }

    #[test]
    fn cartesian_sync_never_completes_when_follower_is_wedged() {
        // The whole point of the redesign: if the follower can't track
        // (e.g. IK keeps failing, motor stalled), the leader-anchor
        // gap may close but the leader-FOLLOWER gap never does, so
        // sync must NEVER engage and pass-through never enables. The
        // router keeps publishing rate-limited targets forever.
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let leader = Pose7 {
            timestamp_ms: 123,
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let stuck_follower = Pose7 {
            timestamp_ms: 100,
            values: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let t0 = std::time::Instant::now();

        // Run for many SYNC_HOLD_DURATIONs without the follower
        // ever moving. The published target ramps toward the leader
        // (tested elsewhere); the live gap never drops below the
        // threshold so sync stays active throughout.
        for ms in (0..5_000).step_by(50) {
            let published = cart_apply(
                &mut sync_state,
                leader,
                stuck_follower,
                t0 + Duration::from_millis(ms as u64),
            );
            // Step branch: the published target must stay within
            // SYNC_MAX_STEP_M of the previous anchor, so it cannot
            // jump to the leader's 1.0 m position.
            assert!(
                published.values[0] < 1.0,
                "wedged-follower must keep the published target rate-limited"
            );
        }
        assert!(
            sync_state.enabled(),
            "sync must NEVER complete while follower is wedged at the start position"
        );
    }

    #[test]
    fn cartesian_sync_holds_when_no_follower_feedback() {
        // Cartesian ramp must also refuse to publish without a follower
        // pose to anchor against.
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let original = Pose7 {
            timestamp_ms: 123,
            values: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let mut command = ForwardedCommand::EndPose(original);
        let outcome = sync_state.apply(&mut command, None);
        assert_eq!(outcome, SyncOutcome::Hold);
        let ForwardedCommand::EndPose(command) = command else {
            panic!("expected pose command");
        };
        assert_eq!(command.values, original.values);
        assert!(sync_state.enabled());
    }

    #[test]
    fn cartesian_sync_passes_through_after_completion_for_big_jumps() {
        // Mirrors the joint case: once cartesian sync completes the
        // loop stops calling apply (because `enabled()` is false), so
        // big leader jumps are forwarded verbatim by the caller.
        let config = cartesian_sync_config();
        let mut sync_state = SyncState::new(&config);
        let SyncState::Cartesian(ref mut cart) = sync_state else {
            panic!("expected cartesian sync state");
        };
        cart.synced = true;
        assert!(!sync_state.enabled());
    }

    // -------------------------------------------------------------------
    // Quaternion helper tests
    // -------------------------------------------------------------------

    #[test]
    fn quat_slerp_takes_shortest_path() {
        // A 270 deg rotation around +Z is the same orientation as
        // -90 deg around +Z; slerp from identity should walk toward
        // the negative-Z direction (the short way) once we align
        // hemispheres.
        let identity = [0.0, 0.0, 0.0, 1.0];
        // For an angle theta rotation around +Z, q = [0, 0, sin(theta/2),
        // cos(theta/2)]. theta = 270 deg => half = 135 deg => cos < 0,
        // so the raw quaternion lies on the opposite hemisphere from
        // identity (dot = cos(135 deg) < 0).
        let half_angle = std::f64::consts::PI * 0.75; // 135 deg
        let raw_target = [0.0, 0.0, half_angle.sin(), half_angle.cos()];
        let aligned = quat_align_shortest(&raw_target, &identity);
        // After alignment the target represents the equivalent -90 deg
        // rotation, so qz should be negative.
        assert!(
            aligned[2] < 0.0,
            "expected aligned qz < 0, got {:?}",
            aligned
        );
        // Slerping halfway should land on -45 deg around +Z, i.e.
        // qz = sin(-22.5 deg).
        let halfway = quat_slerp(&identity, &aligned, 0.5);
        assert!(halfway[2] < 0.0);
        let expected_z = (-std::f64::consts::FRAC_PI_8).sin();
        assert!(
            (halfway[2] - expected_z).abs() < 1e-6,
            "slerp halfway qz {} != expected {}",
            halfway[2],
            expected_z,
        );
    }

    #[test]
    fn clamp_translation_returns_original_distance() {
        let mut target = [1.0, 0.0, 0.0];
        let anchor = [0.0, 0.0, 0.0];
        let dist = clamp_translation(&mut target, &anchor, 0.1);
        assert!((dist - 1.0).abs() < 1e-9);
        assert!((target[0] - 0.1).abs() < 1e-9);
        assert_eq!(target[1], 0.0);
        assert_eq!(target[2], 0.0);
    }

    #[test]
    fn clamp_translation_passes_short_deltas_through() {
        let mut target = [0.05, 0.0, 0.0];
        let anchor = [0.0, 0.0, 0.0];
        let dist = clamp_translation(&mut target, &anchor, 0.1);
        assert!((dist - 0.05).abs() < 1e-9);
        // Within max_step, target untouched.
        assert_eq!(target, [0.05, 0.0, 0.0]);
    }

    #[test]
    fn clamp_rotation_returns_original_angle() {
        let half = std::f64::consts::FRAC_PI_4;
        let mut target_q = [0.0, 0.0, half.sin(), half.cos()]; // 90 deg around +Z
        let anchor_q = [0.0, 0.0, 0.0, 1.0];
        let max_step = 0.1;
        let angle = clamp_rotation(&mut target_q, &anchor_q, max_step);
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        let new_angle = quat_angle_between(&anchor_q, &target_q);
        assert!(new_angle <= max_step + 1e-9);
    }
}
