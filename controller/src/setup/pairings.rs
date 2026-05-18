use super::state::{rotate_index, DiscoveredChannelMeta, SetupSession, TeleopPairCreate};
use rollio_types::config::{
    BinaryDeviceConfig, ChannelPairingConfig, DeviceChannelConfigV2, DeviceType, MappingStrategy,
    RobotCommandKind, RobotMode, RobotStateKind,
};
use std::collections::BTreeMap;
use std::error::Error;

impl SetupSession {
    pub(super) fn cycle_pair_mapping(
        &mut self,
        index: usize,
        delta: i32,
    ) -> Result<bool, Box<dyn Error>> {
        let Some(snapshot) = self.config.pairings.get(index).cloned() else {
            return Ok(false);
        };
        let leader_device = snapshot.leader_device.clone();
        let leader_channel_type = snapshot.leader_channel_type.clone();
        let follower_device = snapshot.follower_device.clone();
        let follower_channel_type = snapshot.follower_channel_type.clone();
        let current_mapping = snapshot.mapping;
        // Cycle order: DirectJoint -> Cartesian -> Parallel -> wrap.
        let options = [
            MappingStrategy::DirectJoint,
            MappingStrategy::Cartesian,
            MappingStrategy::Parallel,
        ];
        let current_index = options
            .iter()
            .position(|mapping| *mapping == current_mapping)
            .unwrap_or(0);
        let next_index = rotate_index(current_index, options.len(), delta);
        let next_mapping = options[next_index];
        if next_mapping == current_mapping {
            return Ok(false);
        }
        // Build the candidate pair from scratch under the new policy.
        // For Parallel, we seed ratio = 1.0 (the operator can fine-tune
        // it via the picker's ratio phase afterwards).
        let new_pair = pairing_from_channels(
            &self.config.devices,
            next_mapping,
            &leader_device,
            &leader_channel_type,
            &follower_device,
            &follower_channel_type,
            None,
        );
        // Snapshot publish_states so we can roll back the EndEffectorPose
        // opt-in for Cartesian. DirectJoint / Parallel use kinds the
        // discovery defaults already include.
        let publish_states_snapshot = self
            .config
            .device_named(&leader_device)
            .and_then(|d| d.channel_named(&leader_channel_type))
            .map(|ch| ch.publish_states.clone());
        self.config.pairings[index] = new_pair;
        let leader_state = self.config.pairings[index].leader_state;
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        if let Err(error) = self.config.validate() {
            self.config.pairings[index] = snapshot;
            if let Some(states) = publish_states_snapshot {
                if let Some(device) = self
                    .config
                    .devices
                    .iter_mut()
                    .find(|d| d.name == leader_device)
                {
                    if let Some(channel) = device
                        .channels
                        .iter_mut()
                        .find(|c| c.channel_type == leader_channel_type)
                    {
                        channel.publish_states = states;
                    }
                }
            }
            self.teleop_pairing_cache = self.config.pairings.clone();
            self.message = Some(format!(
                "Cannot switch to {} mapping: {error}",
                mapping_strategy_label(next_mapping),
            ));
            return Ok(false);
        }
        self.teleop_pairing_cache = self.config.pairings.clone();
        Ok(true)
    }

    /// Build a `(device_name, channel_type)` list of every enabled robot
    /// channel whose driver advertises **either** `FreeDrive` **or**
    /// `CommandFollowing`, with the pair at `except_pair_index` excluded
    /// Policy-aware leader eligibility. Returns the `(device, channel)`
    /// names of every enabled robot channel that satisfies BOTH:
    ///
    ///   1. The shared leader requirement: driver advertises FreeDrive
    ///      or CommandFollowing (so the controller can observe joint
    ///      state -- a passive EEF like AIRBOT E2 with only FreeDrive
    ///      still qualifies because the operator demonstrates motion
    ///      manually).
    ///   2. The per-policy shape predicate (`channel_supports_*_leader`).
    ///
    /// The pair at `except_pair_index` is excluded from the no-self-loop
    /// guard so editing a pair's leader doesn't filter out its current
    /// follower's match (the operator should be able to confirm the
    /// existing pick without it being marked unavailable).
    pub(super) fn eligible_leader_channels_for(
        &self,
        policy: MappingStrategy,
        except_pair_index: Option<usize>,
    ) -> Vec<(String, String)> {
        let blocked_self = except_pair_index
            .and_then(|idx| self.config.pairings.get(idx))
            .map(|pair| {
                (
                    pair.follower_device.clone(),
                    pair.follower_channel_type.clone(),
                )
            });
        let mut out = Vec::new();
        for device in &self.config.devices {
            for channel in &device.channels {
                if !channel.enabled || channel.kind != DeviceType::Robot {
                    continue;
                }
                let supported = self.supported_modes_for(device, channel);
                let leader_capable = supported.contains(&RobotMode::FreeDrive)
                    || supported.contains(&RobotMode::CommandFollowing);
                if !leader_capable {
                    continue;
                }
                let policy_match = match policy {
                    MappingStrategy::DirectJoint => channel_supports_direct_joint_leader(channel),
                    MappingStrategy::Cartesian => channel_supports_cartesian_leader(channel),
                    MappingStrategy::Parallel => channel_supports_parallel_leader(channel),
                };
                if !policy_match {
                    continue;
                }
                let candidate = (device.name.clone(), channel.channel_type.clone());
                if Some(&candidate) == blocked_self.as_ref() {
                    continue;
                }
                out.push(candidate);
            }
        }
        out
    }

    /// Policy-aware follower eligibility. Returns the `(device, channel)`
    /// names of every enabled robot channel that satisfies ALL of:
    ///
    ///   1. The shared follower requirement: driver advertises
    ///      CommandFollowing (so the channel can be driven).
    ///   2. The per-policy shape predicate
    ///      (`channel_supports_*_follower`).
    ///   3. Per-policy peer compatibility against the optional `leader`:
    ///      DirectJoint requires matching DOF AND a mutual driver
    ///      whitelist (`direct_joint_compatibility`); the other policies
    ///      have no peer-shape constraint beyond the predicate.
    ///   4. Cross-pair uniqueness: a follower must not already follow
    ///      another leader (excluding the pair at `except_pair_index`)
    ///      and must not collapse onto its own pair's leader.
    pub(super) fn eligible_follower_channels_for(
        &self,
        policy: MappingStrategy,
        leader: Option<&(String, String)>,
        except_pair_index: Option<usize>,
    ) -> Vec<(String, String)> {
        let blocked_self = except_pair_index
            .and_then(|idx| self.config.pairings.get(idx))
            .map(|pair| (pair.leader_device.clone(), pair.leader_channel_type.clone()));
        let claimed_followers: BTreeMap<(String, String), ()> = self
            .config
            .pairings
            .iter()
            .enumerate()
            .filter_map(|(idx, pair)| {
                if Some(idx) == except_pair_index {
                    return None;
                }
                Some((
                    (
                        pair.follower_device.clone(),
                        pair.follower_channel_type.clone(),
                    ),
                    (),
                ))
            })
            .collect();

        // Resolve the leader's `(BinaryDeviceConfig, DeviceChannelConfigV2)`
        // once so we don't walk the device list inside the loop.
        let leader_resolved = leader.and_then(|(device_name, channel_type)| {
            self.config.devices.iter().find_map(|d| {
                if d.name == *device_name {
                    d.channels
                        .iter()
                        .find(|c| c.channel_type == *channel_type)
                        .map(|ch| (d.clone(), ch.clone()))
                } else {
                    None
                }
            })
        });

        let mut out = Vec::new();
        for device in &self.config.devices {
            for channel in &device.channels {
                if !channel.enabled || channel.kind != DeviceType::Robot {
                    continue;
                }
                let supported = self.supported_modes_for(device, channel);
                if !supported.contains(&RobotMode::CommandFollowing) {
                    continue;
                }
                let policy_match = match policy {
                    MappingStrategy::DirectJoint => channel_supports_direct_joint_follower(channel),
                    MappingStrategy::Cartesian => channel_supports_cartesian_follower(channel),
                    MappingStrategy::Parallel => channel_supports_parallel_follower(channel),
                };
                if !policy_match {
                    continue;
                }
                if let Some((leader_device, leader_channel)) = leader_resolved.as_ref() {
                    if !policy_pair_compatible(
                        policy,
                        leader_device,
                        leader_channel,
                        device,
                        channel,
                    ) {
                        continue;
                    }
                }
                let candidate = (device.name.clone(), channel.channel_type.clone());
                if Some(&candidate) == blocked_self.as_ref() {
                    continue;
                }
                if claimed_followers.contains_key(&candidate) {
                    continue;
                }
                out.push(candidate);
            }
        }
        out
    }

    pub(super) fn supported_modes_for(
        &self,
        device: &BinaryDeviceConfig,
        channel: &DeviceChannelConfigV2,
    ) -> &[RobotMode] {
        self.available_devices
            .iter()
            .find(|available| {
                available.driver == device.driver
                    && available.id == device.id
                    && available
                        .current
                        .channels
                        .first()
                        .is_some_and(|ch| ch.channel_type == channel.channel_type)
            })
            .map(|available| available.supported_modes.as_slice())
            .unwrap_or(&[])
    }

    /// Push a new pair into `config.pairings`. When `explicit` is `Some`,
    /// uses the operator-supplied `{policy, leader, follower, ratio}`;
    /// otherwise picks defaults the same way `build_default_channel_pairings`
    /// does. Returns the new pair's index so the UI can immediately focus
    /// the row.
    ///
    /// The wizard's modal pairing picker invokes this with `explicit =
    /// Some(...)` after the operator has walked through policy -> leader
    /// -> follower -> (ratio for Parallel) sub-steps; deferred creation
    /// means esc at any point during the picker leaves nothing behind.
    pub(super) fn create_pairing(
        &mut self,
        explicit: Option<TeleopPairCreate>,
    ) -> Result<Option<usize>, Box<dyn Error>> {
        let TeleopPairCreate {
            policy,
            leader: (leader_device, leader_channel_type),
            follower: (follower_device, follower_channel_type),
            ratio,
        } = match explicit {
            Some(pair) => pair,
            None => match self.pick_default_pair_endpoints()? {
                Some(pair) => pair,
                None => return Ok(None),
            },
        };

        // Validate eligibility against the live config so an explicit
        // request from the UI can't smuggle a stale or ineligible
        // channel past us (e.g. operator deselected a channel in step 1
        // while the picker was still open). Eligibility is policy-aware:
        // a leader/follower combo that's fine for Cartesian may be
        // rejected for DirectJoint (DOF mismatch, missing whitelist).
        let eligible_leaders = self.eligible_leader_channels_for(policy, None);
        let leader_target = (leader_device.clone(), leader_channel_type.clone());
        if !eligible_leaders.contains(&leader_target) {
            self.message = Some(format!(
                "Leader {}:{} is not eligible for the {} policy (channel may have been disabled in step 1, or no longer satisfies the policy's predicate).",
                leader_device,
                leader_channel_type,
                mapping_strategy_label(policy),
            ));
            return Ok(None);
        }
        let eligible_followers =
            self.eligible_follower_channels_for(policy, Some(&leader_target), None);
        let follower_target = (follower_device.clone(), follower_channel_type.clone());
        if !eligible_followers.contains(&follower_target) {
            self.message = Some(format!(
                "Follower {}:{} is not eligible for the {} policy (channel may have been disabled in step 1, already follow another leader, or fail the policy's predicate).",
                follower_device,
                follower_channel_type,
                mapping_strategy_label(policy),
            ));
            return Ok(None);
        }
        if leader_target == follower_target {
            self.message = Some("Leader and follower channel must differ.".into());
            return Ok(None);
        }

        let pair = pairing_from_channels(
            &self.config.devices,
            policy,
            &leader_device,
            &leader_channel_type,
            &follower_device,
            &follower_channel_type,
            ratio,
        );
        let leader_state = pair.leader_state;
        self.config.pairings.push(pair);
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        // A follower must run in CommandFollowing for the controller to
        // drive it. Auto-promote the channel mode that step 1 set so the
        // operator doesn't have to bounce back to step 1 just to flip it
        // (and so a `FreeDrive`-only mode picked in step 1 doesn't
        // silently break teleop at runtime).
        self.promote_follower_channel_to_command_following(
            &follower_device,
            &follower_channel_type,
        );
        // Teleop is already the only collection mode the wizard exposes;
        // creating a pair doesn't need to mutate `config.mode`.
        self.teleop_pairing_cache = self.config.pairings.clone();
        let new_index = self.config.pairings.len() - 1;
        if let Err(error) = self.config.validate() {
            // Roll back if validation fails (e.g. duplicate pair).
            self.config.pairings.pop();
            self.teleop_pairing_cache = self.config.pairings.clone();
            self.message = Some(format!("Could not create pairing: {error}"));
            return Ok(None);
        }
        Ok(Some(new_index))
    }

    /// Pick a default `{policy, leader, follower}` for a brand-new pair
    /// using the per-policy eligibility filters. Tries the three
    /// policies in priority order (Parallel for grippers, then
    /// DirectJoint, then Cartesian) and returns the first combo that
    /// passes. Used as the fallback when the UI doesn't supply explicit
    /// endpoints (e.g. the legacy / test entry point for create_pairing).
    pub(super) fn pick_default_pair_endpoints(
        &mut self,
    ) -> Result<Option<TeleopPairCreate>, Box<dyn Error>> {
        for policy in [
            MappingStrategy::Parallel,
            MappingStrategy::DirectJoint,
            MappingStrategy::Cartesian,
        ] {
            let leaders = self.eligible_leader_channels_for(policy, None);
            if leaders.is_empty() {
                continue;
            }
            for leader in &leaders {
                let followers = self.eligible_follower_channels_for(policy, Some(leader), None);
                for follower in &followers {
                    if leader != follower {
                        return Ok(Some(TeleopPairCreate {
                            policy,
                            leader: leader.clone(),
                            follower: follower.clone(),
                            ratio: matches!(policy, MappingStrategy::Parallel).then_some(1.0),
                        }));
                    }
                }
            }
        }
        self.message = Some(
            "No eligible leader / follower combination found. Pick a policy whose predicate at least two of the selected robot channels satisfy.".into(),
        );
        Ok(None)
    }

    pub(super) fn remove_pairing(&mut self, index: usize) -> Result<bool, Box<dyn Error>> {
        if index >= self.config.pairings.len() {
            return Ok(false);
        }
        self.config.pairings.remove(index);
        // Stay in teleop even when pairings reach zero: teleop is the only
        // mode the wizard exposes now, and the operator is expected to
        // create a new pair via `m` before saving for runtime use.
        self.teleop_pairing_cache = self.config.pairings.clone();
        self.config.validate()?;
        Ok(true)
    }

    pub(super) fn set_pairing_endpoint(
        &mut self,
        index: usize,
        endpoint: PairingEndpoint,
        device: &str,
        channel_type: &str,
    ) -> Result<bool, Box<dyn Error>> {
        if index >= self.config.pairings.len() {
            return Ok(false);
        }
        let policy = self.config.pairings[index].mapping;
        let snapshot_ratio = if policy == MappingStrategy::Parallel {
            self.config.pairings[index].joint_scales.first().copied()
        } else {
            None
        };
        // Pass the targeted pair index so the eligibility check excludes
        // *this* pair's existing endpoint from the no-self-loop /
        // uniqueness filters. The policy is fixed (edit doesn't change
        // it -- `h/l` cycle handles policy changes separately), so we
        // filter against the same predicate the validator will apply.
        let eligible = match endpoint {
            PairingEndpoint::Leader => self.eligible_leader_channels_for(policy, Some(index)),
            PairingEndpoint::Follower => {
                let other = (
                    self.config.pairings[index].leader_device.clone(),
                    self.config.pairings[index].leader_channel_type.clone(),
                );
                self.eligible_follower_channels_for(policy, Some(&other), Some(index))
            }
        };
        let target = (device.to_owned(), channel_type.to_owned());
        if !eligible.contains(&target) {
            self.message = Some(format!(
                "{} {}:{} is not eligible for the {} policy (channel may not satisfy the predicate, may collide with this pair's other endpoint, or may already be a follower in another pair).",
                match endpoint {
                    PairingEndpoint::Leader => "Leader",
                    PairingEndpoint::Follower => "Follower",
                },
                device,
                channel_type,
                mapping_strategy_label(policy),
            ));
            return Ok(false);
        }
        let (leader_device, leader_channel_type, follower_device, follower_channel_type) = {
            let pair = &mut self.config.pairings[index];
            match endpoint {
                PairingEndpoint::Leader => {
                    pair.leader_device = device.to_owned();
                    pair.leader_channel_type = channel_type.to_owned();
                }
                PairingEndpoint::Follower => {
                    pair.follower_device = device.to_owned();
                    pair.follower_channel_type = channel_type.to_owned();
                }
            }
            (
                pair.leader_device.clone(),
                pair.leader_channel_type.clone(),
                pair.follower_device.clone(),
                pair.follower_channel_type.clone(),
            )
        };
        // Re-derive state/command kinds (and the parallel ratio) from
        // the new endpoint while keeping the policy fixed. For Parallel
        // we preserve the existing ratio so the operator's tuning isn't
        // wiped when they swap a follower.
        let rebuilt = pairing_from_channels(
            &self.config.devices,
            policy,
            &leader_device,
            &leader_channel_type,
            &follower_device,
            &follower_channel_type,
            snapshot_ratio,
        );
        self.config.pairings[index] = rebuilt;
        let leader_state = self.config.pairings[index].leader_state;
        ensure_channel_publishes_state(
            &mut self.config.devices,
            &leader_device,
            &leader_channel_type,
            leader_state,
        );
        // Same auto-promotion as `create_pairing`: a follower must run
        // in CommandFollowing. Editing either endpoint of the pair can
        // shift the follower (directly when `endpoint == Follower`, or
        // by binding a previously-leading channel as a new follower
        // elsewhere via the rebuild), so always normalize the current
        // follower channel.
        self.promote_follower_channel_to_command_following(
            &follower_device,
            &follower_channel_type,
        );
        self.teleop_pairing_cache = self.config.pairings.clone();
        self.config.validate()?;
        Ok(true)
    }

    /// Force the named robot channel into `CommandFollowing` mode, both
    /// in the persisted config (the source of truth used by `validate`
    /// and the runtime) and in the `available_devices` mirror that
    /// powers step 1's UI -- so the operator sees the change reflected
    /// next time they revisit step 1. No-op for non-robot channels and
    /// for drivers that don't advertise `CommandFollowing` (the
    /// eligibility filter blocks those before we get here, but stay
    /// defensive in case the available_devices snapshot is stale).
    pub(super) fn promote_follower_channel_to_command_following(
        &mut self,
        device_name: &str,
        channel_type: &str,
    ) {
        if let Some(device) = self
            .config
            .devices
            .iter_mut()
            .find(|d| d.name == device_name)
        {
            if let Some(channel) = device
                .channels
                .iter_mut()
                .find(|c| c.channel_type == channel_type)
            {
                if channel.kind == DeviceType::Robot {
                    channel.mode = Some(RobotMode::CommandFollowing);
                }
            }
        }
        if let Some(available) = self.available_device_mut(device_name) {
            if let Some(channel) = available.current.channels.first_mut() {
                if channel.kind == DeviceType::Robot
                    && channel.channel_type == channel_type
                    && available
                        .supported_modes
                        .contains(&RobotMode::CommandFollowing)
                {
                    channel.mode = Some(RobotMode::CommandFollowing);
                }
            }
        }
    }

    /// Mutate the parallel-ratio (joint_scales[0]) of an existing
    /// `Parallel` pair. Returns false (with `self.message` set) when the
    /// pair isn't `Parallel` or the supplied ratio is non-finite / zero.
    pub(super) fn set_pairing_ratio(
        &mut self,
        index: usize,
        ratio: f64,
    ) -> Result<bool, Box<dyn Error>> {
        let Some(pair) = self.config.pairings.get(index) else {
            return Ok(false);
        };
        if pair.mapping != MappingStrategy::Parallel {
            self.message = Some(format!(
                "Pair {}:{} -> {}:{} uses {} mapping, which has no ratio. Cycle to parallel via h/l first.",
                pair.leader_device,
                pair.leader_channel_type,
                pair.follower_device,
                pair.follower_channel_type,
                mapping_strategy_label(pair.mapping),
            ));
            return Ok(false);
        }
        if !ratio.is_finite() || ratio == 0.0 {
            self.message = Some(format!(
                "Parallel ratio must be finite and non-zero (got {ratio})."
            ));
            return Ok(false);
        }
        let snapshot = self.config.pairings[index].joint_scales.clone();
        self.config.pairings[index].joint_scales = vec![ratio];
        if let Err(error) = self.config.validate() {
            self.config.pairings[index].joint_scales = snapshot;
            self.message = Some(format!("Cannot apply ratio: {error}"));
            return Ok(false);
        }
        self.teleop_pairing_cache = self.config.pairings.clone();
        Ok(true)
    }
}

pub(super) fn mapping_strategy_label(policy: MappingStrategy) -> &'static str {
    match policy {
        MappingStrategy::DirectJoint => "direct-joint",
        MappingStrategy::Cartesian => "cartesian",
        MappingStrategy::Parallel => "parallel",
    }
}

/// Per-policy peer compatibility check used by the picker to decide
/// whether `(leader, follower)` can be paired under the given policy
/// before building the actual `ChannelPairingConfig`. Mirrors the
/// `ChannelPairingConfig::validate` checks but operates on the channels
/// directly so the wizard can filter follower options by leader without
/// constructing a transient pair.
///
///   - `DirectJoint`: leader.dof == follower.dof, AND both drivers
///     opt in via `direct_joint_compatibility` (two-sided whitelist).
///   - `Cartesian`: no extra constraint (the per-channel predicates
///     already covered the state/command requirements).
///   - `Parallel`: both channels are dof=1 by predicate; no further
///     compatibility check.
pub(super) fn policy_pair_compatible(
    policy: MappingStrategy,
    leader_device: &BinaryDeviceConfig,
    leader_channel: &DeviceChannelConfigV2,
    follower_device: &BinaryDeviceConfig,
    follower_channel: &DeviceChannelConfigV2,
) -> bool {
    match policy {
        MappingStrategy::DirectJoint => {
            if leader_channel.dof.is_none() || leader_channel.dof != follower_channel.dof {
                return false;
            }
            let leader_endorses = leader_channel
                .direct_joint_compatibility
                .can_lead
                .iter()
                .any(|peer| {
                    peer.driver == follower_device.driver
                        && peer.channel_type == follower_channel.channel_type
                });
            let follower_endorses = follower_channel
                .direct_joint_compatibility
                .can_follow
                .iter()
                .any(|peer| {
                    peer.driver == leader_device.driver
                        && peer.channel_type == leader_channel.channel_type
                });
            leader_endorses && follower_endorses
        }
        MappingStrategy::Cartesian | MappingStrategy::Parallel => true,
    }
}

pub(super) fn channel_supports_direct_joint_leader(ch: &DeviceChannelConfigV2) -> bool {
    ch.publish_states.contains(&RobotStateKind::JointPosition) && ch.dof.is_some_and(|d| d > 0)
}

pub(super) fn channel_supports_direct_joint_follower(ch: &DeviceChannelConfigV2) -> bool {
    ch.supported_commands
        .contains(&RobotCommandKind::JointPosition)
        && ch.dof.is_some_and(|d| d > 0)
}

pub(super) fn channel_supports_parallel_leader(ch: &DeviceChannelConfigV2) -> bool {
    ch.dof == Some(1)
        && ch
            .publish_states
            .contains(&RobotStateKind::ParallelPosition)
}

pub(super) fn channel_supports_parallel_follower(ch: &DeviceChannelConfigV2) -> bool {
    ch.dof == Some(1)
        && (ch
            .supported_commands
            .contains(&RobotCommandKind::ParallelPosition)
            || ch
                .supported_commands
                .contains(&RobotCommandKind::ParallelMit))
}

/// Pick the most natural default policy for a freshly seeded
/// `(leader, follower)` pair based on the channels' shape. Used by the
/// auto-build path during discovery and by the wizard when the operator
/// asks for a default mapping. Returns `None` when no policy is
/// applicable; the wizard surfaces a helpful message in that case.
pub(super) fn default_mapping_strategy_for(
    leader: &DeviceChannelConfigV2,
    follower: &DeviceChannelConfigV2,
) -> Option<MappingStrategy> {
    if channel_supports_parallel_leader(leader) && channel_supports_parallel_follower(follower) {
        return Some(MappingStrategy::Parallel);
    }
    if channel_supports_direct_joint_leader(leader)
        && channel_supports_direct_joint_follower(follower)
        && leader.dof == follower.dof
    {
        return Some(MappingStrategy::DirectJoint);
    }
    if channel_supports_cartesian_leader(leader) && channel_supports_cartesian_follower(follower) {
        return Some(MappingStrategy::Cartesian);
    }
    None
}

/// Pick the default `publish_states` for a freshly discovered robot
/// channel. Prefer whatever the driver advertises (`supported_states`,
/// falling back to the kinds enumerated by `value_limits`) so newly added
/// state kinds (e.g. `EndEffectorPose` on the airbot arm) are turned on
/// without requiring a config edit. Falls back to a static template when
/// the driver query returned nothing usable, so older drivers and tests
/// keep working unchanged.
pub(super) fn default_publish_states_for_meta(
    meta: &DiscoveredChannelMeta,
    fallback: &[RobotStateKind],
) -> Vec<RobotStateKind> {
    if !meta.supported_states.is_empty() {
        return dedup_in_order(&meta.supported_states);
    }
    let from_limits: Vec<RobotStateKind> = meta
        .value_limits
        .iter()
        .map(|entry| entry.state_kind)
        .collect();
    if !from_limits.is_empty() {
        return dedup_in_order(&from_limits);
    }
    fallback.to_vec()
}

pub(super) fn dedup_in_order(values: &[RobotStateKind]) -> Vec<RobotStateKind> {
    let mut out: Vec<RobotStateKind> = Vec::with_capacity(values.len());
    for value in values {
        if !out.contains(value) {
            out.push(*value);
        }
    }
    out
}

/// Ensure a robot channel publishes the given state kind. Used as a
/// safety net when switching pairings to FK/IK so we don't blow up on
/// validation if a legacy config (or an operator who toggled the kind off
/// in the new "States" sub-step) doesn't have it. Newly discovered
/// channels already opt every supported kind into `publish_states` via
/// `default_publish_states_for_meta`, so this rarely runs in fresh
/// projects.
pub(super) fn ensure_channel_publishes_state(
    devices: &mut [BinaryDeviceConfig],
    device_name: &str,
    channel_type: &str,
    state: RobotStateKind,
) {
    let Some(device) = devices.iter_mut().find(|d| d.name == device_name) else {
        return;
    };
    let Some(channel) = device
        .channels
        .iter_mut()
        .find(|c| c.channel_type == channel_type)
    else {
        return;
    };
    if !channel.publish_states.contains(&state) {
        channel.publish_states.push(state);
    }
}

pub(super) fn build_default_channel_pairings(
    devices: &[BinaryDeviceConfig],
) -> Vec<ChannelPairingConfig> {
    let mut pairings = Vec::new();
    let arms = primary_robot_channels(devices, false);
    let eefs = primary_robot_channels(devices, true);
    for pairs in [
        pair_robot_channels_by_order(&arms),
        pair_robot_channels_by_order(&eefs),
    ] {
        let Some((leader_dev, leader_ch, follower_dev, follower_ch)) = pairs else {
            continue;
        };
        let Some(policy) = default_mapping_strategy_for(leader_ch, follower_ch) else {
            continue;
        };
        // For DirectJoint, also require the two-sided whitelist before
        // producing the auto-pair. We don't want discovery to seed an
        // invalid pair the operator must immediately delete.
        if policy == MappingStrategy::DirectJoint
            && !policy_pair_compatible(policy, leader_dev, leader_ch, follower_dev, follower_ch)
        {
            continue;
        }
        pairings.push(pairing_from_channels(
            devices,
            policy,
            &leader_dev.name,
            &leader_ch.channel_type,
            &follower_dev.name,
            &follower_ch.channel_type,
            None,
        ));
    }
    pairings
}

/// Endpoint of a `ChannelPairingConfig` selected by the wizard's manual
/// pairing flow. Used by `set_pairing_endpoint` to know which side of an
/// existing pair to mutate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PairingEndpoint {
    Leader,
    Follower,
}

/// Build a single `ChannelPairingConfig` for an explicit `policy`. The
/// caller chose the policy; this just fills the state/command kinds and
/// the joint_index_map / joint_scales the way the corresponding
/// validator expects. `validate()` on the parent ProjectConfig will
/// surface any remaining shape mismatch (e.g. DOFs differ for a
/// DirectJoint pair). The optional `ratio` is only meaningful for
/// `Parallel`; ignored otherwise.
pub(super) fn pairing_from_channels(
    devices: &[BinaryDeviceConfig],
    policy: MappingStrategy,
    leader_device: &str,
    leader_channel_type: &str,
    follower_device: &str,
    follower_channel_type: &str,
    ratio: Option<f64>,
) -> ChannelPairingConfig {
    let follower_ch = devices
        .iter()
        .find(|d| d.name == follower_device)
        .and_then(|d| {
            d.channels
                .iter()
                .find(|c| c.channel_type == follower_channel_type)
        });
    let follower_dof = follower_ch.and_then(|ch| ch.dof).unwrap_or(0);

    match policy {
        MappingStrategy::DirectJoint => ChannelPairingConfig {
            leader_device: leader_device.to_owned(),
            leader_channel_type: leader_channel_type.to_owned(),
            follower_device: follower_device.to_owned(),
            follower_channel_type: follower_channel_type.to_owned(),
            mapping: MappingStrategy::DirectJoint,
            leader_state: RobotStateKind::JointPosition,
            follower_command: RobotCommandKind::JointPosition,
            joint_index_map: (0..follower_dof).collect(),
            joint_scales: vec![1.0; follower_dof as usize],
        },
        MappingStrategy::Cartesian => ChannelPairingConfig {
            leader_device: leader_device.to_owned(),
            leader_channel_type: leader_channel_type.to_owned(),
            follower_device: follower_device.to_owned(),
            follower_channel_type: follower_channel_type.to_owned(),
            mapping: MappingStrategy::Cartesian,
            leader_state: RobotStateKind::EndEffectorPose,
            follower_command: RobotCommandKind::EndPose,
            joint_index_map: Vec::new(),
            joint_scales: Vec::new(),
        },
        MappingStrategy::Parallel => {
            // Prefer ParallelMit when the follower advertises it
            // (matches today's gripper behaviour with kp/kd from
            // command_defaults); fall back to ParallelPosition otherwise.
            let follower_command = if follower_ch.is_some_and(|ch| {
                ch.supported_commands
                    .contains(&RobotCommandKind::ParallelMit)
            }) {
                RobotCommandKind::ParallelMit
            } else {
                RobotCommandKind::ParallelPosition
            };
            let ratio = ratio.filter(|r| r.is_finite() && *r != 0.0).unwrap_or(1.0);
            ChannelPairingConfig {
                leader_device: leader_device.to_owned(),
                leader_channel_type: leader_channel_type.to_owned(),
                follower_device: follower_device.to_owned(),
                follower_channel_type: follower_channel_type.to_owned(),
                mapping: MappingStrategy::Parallel,
                leader_state: RobotStateKind::ParallelPosition,
                follower_command,
                joint_index_map: Vec::new(),
                joint_scales: vec![ratio],
            }
        }
    }
}

pub(super) fn channel_supports_cartesian_leader(ch: &DeviceChannelConfigV2) -> bool {
    ch.publish_states.contains(&RobotStateKind::EndEffectorPose)
}

pub(super) fn channel_supports_cartesian_follower(ch: &DeviceChannelConfigV2) -> bool {
    ch.supported_commands.contains(&RobotCommandKind::EndPose)
}

pub(super) fn primary_robot_channels(
    devices: &[BinaryDeviceConfig],
    end_effector_only: bool,
) -> Vec<(&BinaryDeviceConfig, &DeviceChannelConfigV2)> {
    devices
        .iter()
        .filter_map(|device| {
            let ch = device.channels.iter().find(|c| {
                c.kind == DeviceType::Robot
                    && c.enabled
                    && ((c.dof == Some(1)) == end_effector_only)
            })?;
            Some((device, ch))
        })
        .collect()
}

pub(super) fn pair_robot_channels_by_order<'a>(
    channels: &[(&'a BinaryDeviceConfig, &'a DeviceChannelConfigV2)],
) -> Option<(
    &'a BinaryDeviceConfig,
    &'a DeviceChannelConfigV2,
    &'a BinaryDeviceConfig,
    &'a DeviceChannelConfigV2,
)> {
    match channels {
        [a, b, ..] => Some((a.0, a.1, b.0, b.1)),
        _ => None,
    }
}
