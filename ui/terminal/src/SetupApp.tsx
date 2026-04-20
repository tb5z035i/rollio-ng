import React, { useEffect, useMemo, useState } from "react";
import { Box, Text, useInput, useStdin, useStdout } from "ink";
import { TitleBar } from "./components/TitleBar.js";
import { SetupStatusBar } from "./components/SetupStatusBar.js";
import { LivePreviewPanels } from "./components/LivePreviewPanels.js";
import { KeyHintsBar, type KeyHint } from "./components/KeyHintsBar.js";
import { useControlSocket, usePreviewSocket } from "./lib/websocket.js";
import {
  encodeSetupCommand,
  type CommandAction,
  type SetupAvailableDevice,
  type SetupBinaryDeviceConfig,
  type SetupDeviceChannelV2,
  type SetupStateMessage,
} from "./lib/protocol.js";
import { getTerminalMetrics } from "./lib/terminal-geometry.js";
import {
  getAsciiRendererLabel,
  nextAsciiRendererId,
  type AsciiRendererId,
} from "./lib/renderers/index.js";

type StaticEditableFieldId =
  | "project_name"
  | "storage_output_path"
  | "storage_endpoint"
  | "ui_http_host";
type EditableFieldId = StaticEditableFieldId | `device_name:${string}`;

type SettingsField = {
  id: string;
  label: string;
  value: string;
  kind: "text" | "cycle";
  action?: Extract<
    CommandAction,
    | "setup_cycle_collection_mode"
    | "setup_cycle_episode_format"
    | "setup_cycle_video_codec"
    | "setup_cycle_depth_codec"
    | "setup_cycle_storage_backend"
  >;
  editableFieldId?: EditableFieldId;
};

type PreviewJumpStep = "devices" | "states" | "storage" | "pairing";

type PreviewAction =
  | { kind: "jump"; label: string; targetStep: PreviewJumpStep }
  | { kind: "save"; label: string };

/** Two-phase modal picker state for the pairing step. While non-null, the
 *  global key handler hijacks j/k/enter/esc to scroll/commit/cancel the
 *  picker rather than acting on the underlying detail row.
 *
 *  - `kind`: `"edit"` mutates an existing pair via setup_set_pairing_*;
 *    `"create"` defers any controller mutation until the operator
 *    confirms BOTH endpoints, then sends a single setup_create_pairing.
 *    Esc in `"create"` mode silently drops the draft — no pair is born.
 *  - `index`: target pair (edit only).
 *  - `draftLeader`: leader confirmed in the first phase of a `"create"`
 *    draft, carried forward into the follower phase.
 *  - `phase`: which side of the pair we're picking next.
 *  - `cursor`: highlighted row in the candidate list.
 */
type PairingDraft =
  | {
      kind: "edit";
      index: number;
      phase: "leader" | "follower";
      cursor: number;
    }
  | {
      kind: "create";
      phase: "leader" | "follower";
      cursor: number;
      draftLeader?: PairChannelOption;
    };

/** One toggleable row in the "States" sub-step: a single (channel, state)
 *  pair across every selected robot channel. */
type StateRow = {
  /** Per-channel `available_devices` key (e.g. "robot|airbot-play|<id>|arm|-").
   *  Used as the `name` parameter for the toggle commands. */
  deviceName: string;
  /** Operator-facing label: "{device}/{channel}". */
  channelLabel: string;
  /** Driver-advertised state kind (serialized as the wire format the
   *  backend expects, e.g. "joint_position"). */
  stateKind: string;
  isPublished: boolean;
  isRecorded: boolean;
};

type DetailSpan = {
  text: string;
  color?: string;
  bold?: boolean;
  dimColor?: boolean;
};

type DetailLine = {
  key: string;
  spans: DetailSpan[];
};

function useTerminalMetrics() {
  const { stdout } = useStdout();
  const [metrics, setMetrics] = useState(() => getTerminalMetrics(stdout));

  useEffect(() => {
    const onResize = () => {
      setMetrics(getTerminalMetrics(stdout));
    };

    onResize();
    stdout.on("resize", onResize);
    return () => {
      stdout.off("resize", onResize);
    };
  }, [stdout]);

  return metrics;
}

type SetupAppProps = {
  controlWebsocketUrl: string;
  previewWebsocketUrl: string;
  initialAsciiRendererId: AsciiRendererId;
};

export function SetupApp({
  controlWebsocketUrl,
  previewWebsocketUrl,
  initialAsciiRendererId,
}: SetupAppProps) {
  const { columns, rows, cellGeometry } = useTerminalMetrics();
  const { isRawModeSupported } = useStdin();
  const supportsInteractiveInput = isRawModeSupported === true;
  const {
    connected: controlConnected,
    send: sendControl,
    setupState,
  } = useControlSocket(controlWebsocketUrl);
  // Only attempt the preview socket while the controller is expected to have
  // a visualizer running (identify on, or step Preview). This avoids the
  // wizard escalating reconnect backoff for many seconds before identify is
  // pressed (cf. debug session 8d351b H6).
  const previewExpected = useMemo(
    () =>
      setupState?.step === "preview" ||
      (setupState?.step === "devices" && setupState.identify_device != null),
    [setupState],
  );
  const {
    connected: previewConnected,
    send: sendPreview,
    frames,
    robotChannels,
    streamInfo,
  } = usePreviewSocket(previewWebsocketUrl, previewExpected);
  // The wizard is "connected" in the user's eyes if the control plane is up;
  // preview comes and goes with identify/preview steps.
  const connected = controlConnected;
  const [cameraRendererId, setCameraRendererId] = useState<AsciiRendererId>(
    initialAsciiRendererId,
  );
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [editingField, setEditingField] = useState<EditableFieldId | null>(null);
  const [draftValue, setDraftValue] = useState("");
  const [pairingDraft, setPairingDraft] = useState<PairingDraft | null>(null);

  useEffect(() => {
    if (!connected) {
      return;
    }
    sendControl(encodeSetupCommand("setup_get_state"));
    const interval = setInterval(() => {
      sendControl(encodeSetupCommand("setup_get_state"));
    }, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [connected, sendControl]);

  const selectedDevices = useMemo(
    () => setupState?.config.devices ?? [],
    [setupState],
  );
  const selectedDeviceKeys = useMemo(
    () => new Set(enabledChannelIdentityKeys(selectedDevices)),
    [selectedDevices],
  );
  const identifyDevice = useMemo(
    () =>
      setupState?.available_devices.find(
        (device) => device.name === setupState.identify_device,
      ) ?? null,
    [setupState],
  );
  const settingsFields = useMemo(
    () => buildSettingsFields(setupState),
    [setupState],
  );
  const previewActions = useMemo(
    () => buildPreviewActions(setupState),
    [setupState],
  );
  const stateRows = useMemo(() => buildStateRows(setupState), [setupState]);
  // Picker option lists are computed against the *targeted* pair (edit
  // mode only) so the no-self-loop and follower-uniqueness rules behave
  // correctly when the operator re-picks an existing pair's endpoint
  // (the pair's own current values shouldn't filter themselves out).
  // For create mode the picker has no pair yet — pass `undefined` so the
  // candidate set reflects the full live pairing graph.
  const pickerExceptIndex =
    pairingDraft?.kind === "edit" ? pairingDraft.index : undefined;
  // Leader options are independent of the picked leader (the create-mode
  // follower phase exclude-self check is applied below at render-time).
  const leaderOptions = useMemo(
    () => eligibleLeaderOptions(setupState, pickerExceptIndex),
    [pickerExceptIndex, setupState],
  );
  const baseFollowerOptions = useMemo(
    () => eligibleFollowerOptions(setupState, pickerExceptIndex),
    [pickerExceptIndex, setupState],
  );
  // In create mode's follower phase, additionally exclude the leader the
  // operator just picked so the picker can't suggest a self-loop.
  const followerOptions = useMemo(() => {
    if (pairingDraft?.kind !== "create" || !pairingDraft.draftLeader) {
      return baseFollowerOptions;
    }
    const leader = pairingDraft.draftLeader;
    return baseFollowerOptions.filter(
      (option) =>
        option.deviceName !== leader.deviceName ||
        option.channelType !== leader.channelType,
    );
  }, [baseFollowerOptions, pairingDraft]);
  const focusableCount = useMemo(() => {
    switch (setupState?.step) {
      case "devices":
        return setupState.available_devices.length;
      case "states":
        return stateRows.length;
      case "pairing":
        // +1 for the trailing virtual `[+ new pair]` row so the operator
        // can focus it and press `m` to create a new pair (keeping the
        // create flow visible/discoverable instead of buried behind a
        // bare keystroke).
        return setupState.config.pairings.length + 1;
      case "storage":
        return settingsFields.length;
      case "preview":
        return previewActions.length;
      default:
        return 0;
    }
  }, [previewActions.length, settingsFields.length, setupState, stateRows.length]);

  useEffect(() => {
    if (focusableCount <= 0) {
      setFocusedIndex(0);
      return;
    }
    setFocusedIndex((current) => Math.min(current, focusableCount - 1));
  }, [focusableCount, setupState?.step]);

  useEffect(() => {
    if (
      setupState?.step !== "storage" &&
      setupState?.step !== "devices" &&
      editingField !== null
    ) {
      setEditingField(null);
      setDraftValue("");
    }
  }, [editingField, setupState?.step]);

  useEffect(() => {
    if (pairingDraft === null) return;
    if (setupState?.step !== "pairing") {
      setPairingDraft(null);
      return;
    }
    // Edit mode targets a real pair: close the picker if the pair was
    // removed by another peer (or the index drifted out of bounds).
    // Create mode is purely UI-side until the operator commits both
    // endpoints, so the pair count is irrelevant.
    if (pairingDraft.kind === "edit") {
      const pairCount = setupState.config.pairings.length;
      if (pairingDraft.index >= pairCount) {
        setPairingDraft(null);
      }
    }
  }, [pairingDraft, setupState?.step, setupState?.config.pairings.length]);

  const rendererLabel = getAsciiRendererLabel(cameraRendererId);
  const detailLines = useMemo(
    () =>
      buildDetailLines(
        setupState,
        focusedIndex,
        selectedDeviceKeys,
        settingsFields,
        previewActions,
        editingField,
        draftValue,
        stateRows,
        pairingDraft,
        leaderOptions,
        followerOptions,
      ),
    [
      draftValue,
      editingField,
      focusedIndex,
      selectedDeviceKeys,
      previewActions,
      settingsFields,
      setupState,
      stateRows,
      pairingDraft,
      leaderOptions,
      followerOptions,
    ],
  );
  const showLivePanels = useMemo(
    () =>
      setupState?.step === "preview" ||
      (setupState?.step === "devices" && setupState.identify_device != null),
    [setupState],
  );
  const livePanelsKey = useMemo(() => {
    if (!setupState) {
      return "waiting";
    }
    if (setupState.step === "devices") {
      return `devices:${setupState.identify_device ?? "idle"}`;
    }
    return `preview:${enabledChannelNames(selectedDevices).join("|")}`;
  }, [selectedDevices, setupState]);
  const livePanelRows = useMemo(() => {
    if (!showLivePanels) {
      return 0;
    }
    // Layout below the camera panel: detail lines + KeyHintsBar (1 row) +
    // SetupStatusBar (1 row). Subtract them from the available rows so
    // the camera area stops short of overdrawing those bars.
    return Math.max(8, rows - 2 - detailLines.length - 2);
  }, [detailLines.length, rows, showLivePanels]);
  const preferredLiveCameraNames = useMemo(() => {
    if (!setupState) {
      return [];
    }
    if (setupState.step === "preview") {
      return enabledCameraNames(selectedDevices);
    }
    if (
      setupState.step === "devices" &&
      identifyDevice?.device_type === "camera"
    ) {
      const channel = primaryChannel(identifyDevice.current);
      return [
        channel
          ? `${identifyDevice.current.name}/${channel.channel_type}`
          : identifyDevice.current.name,
      ];
    }
    return [];
  }, [identifyDevice, selectedDevices, setupState]);
  const keyHints = useMemo(
    () =>
      buildSetupKeyHints({
        setupState,
        editingField,
        pairingDraft,
        previewActionCount: previewActions.length,
        showLivePanels,
        rendererLabel,
      }),
    [
      editingField,
      pairingDraft,
      previewActions.length,
      rendererLabel,
      setupState,
      showLivePanels,
    ],
  );

  useInput(
    (input, key) => {
      if (!setupState || key.ctrl || key.meta) {
        return;
      }

      if (editingField !== null) {
        if (key.escape) {
          setEditingField(null);
          setDraftValue("");
          return;
        }
        if (key.return) {
          const deviceNameKey = deviceNameFieldKey(editingField);
          if (deviceNameKey) {
            sendControl(
              encodeSetupCommand("setup_set_device_name", {
                name: deviceNameKey,
                value: draftValue,
              }),
            );
          } else {
            sendControl(
              encodeSetupCommand(
                editCommandForField(editingField as StaticEditableFieldId),
                {
                value: draftValue,
                },
              ),
            );
          }
          setEditingField(null);
          setDraftValue("");
          return;
        }
        if (key.backspace || key.delete) {
          setDraftValue((current) => current.slice(0, -1));
          return;
        }
        if (input.length > 0 && !key.tab) {
          setDraftValue((current) => current + input);
        }
        return;
      }

      // Pairing picker hijacks all navigation / commit / cancel keys while
      // open; the underlying focus list and step navigation stay frozen.
      if (pairingDraft !== null && setupState.step === "pairing") {
        const options =
          pairingDraft.phase === "leader" ? leaderOptions : followerOptions;
        const normalizedPicker = input.toLowerCase();
        // Accept several "cancel" inputs because some terminals/OSes
        // delay or swallow the bare `\x1b` byte that Ink reads as
        // `key.escape`. `q` and `m` (m toggles back the picker the same
        // key opened it with) are reliable fallbacks in any terminal.
        if (
          key.escape ||
          input === "\u001b" ||
          normalizedPicker === "q" ||
          normalizedPicker === "m"
        ) {
          setPairingDraft(null);
          return;
        }
        if (options.length === 0) {
          // No candidates available. The escape-fallback keys above are
          // already handled; here we additionally accept Enter as a
          // synonym for "close" so the operator can dismiss the empty
          // picker without remembering the cancel keys.
          if (key.return) {
            setPairingDraft(null);
          }
          return;
        }
        if (key.upArrow || normalizedPicker === "k") {
          setPairingDraft((draft) =>
            draft === null
              ? draft
              : { ...draft, cursor: (draft.cursor + options.length - 1) % options.length },
          );
          return;
        }
        if (key.downArrow || normalizedPicker === "j") {
          setPairingDraft((draft) =>
            draft === null ? draft : { ...draft, cursor: (draft.cursor + 1) % options.length },
          );
          return;
        }
        if (key.return) {
          const choice = options[pairingDraft.cursor];
          if (!choice) {
            return;
          }
          if (pairingDraft.kind === "edit") {
            // Edit mode mutates the targeted pair on the controller as
            // each endpoint is committed.
            const action =
              pairingDraft.phase === "leader"
                ? "setup_set_pairing_leader"
                : "setup_set_pairing_follower";
            sendControl(
              encodeSetupCommand(action, {
                index: pairingDraft.index,
                value: `${choice.deviceName}|${choice.channelType}`,
              }),
            );
            if (pairingDraft.phase === "leader") {
              // Advance to the follower phase. `eligibleFollowerOptions`
              // already excludes the targeted pair's leader, so the
              // cursor can simply land on the first option.
              setPairingDraft({
                kind: "edit",
                index: pairingDraft.index,
                phase: "follower",
                cursor: 0,
              });
            } else {
              setPairingDraft(null);
            }
          } else {
            // Create mode: hold the operator's picks UI-side until both
            // are confirmed, then send a single setup_create_pairing.
            // This way, esc at any point silently drops the draft —
            // nothing is created on the controller.
            if (pairingDraft.phase === "leader") {
              setPairingDraft({
                kind: "create",
                phase: "follower",
                cursor: 0,
                draftLeader: choice,
              });
            } else if (pairingDraft.draftLeader) {
              const leader = pairingDraft.draftLeader;
              sendControl(
                encodeSetupCommand("setup_create_pairing", {
                  value: `${leader.deviceName}|${leader.channelType};${choice.deviceName}|${choice.channelType}`,
                }),
              );
              setPairingDraft(null);
            } else {
              // Defensive: missing leader during follower phase → restart.
              setPairingDraft({ kind: "create", phase: "leader", cursor: 0 });
            }
          }
          return;
        }
        return;
      }

      const normalizedInput = input.toLowerCase();
      if (normalizedInput === "r") {
        setCameraRendererId((current) => nextAsciiRendererId(current));
        return;
      }
      if (normalizedInput === "q") {
        sendControl(encodeSetupCommand("setup_cancel"));
        return;
      }
      if (key.upArrow || normalizedInput === "k") {
        setFocusedIndex((current) =>
          focusableCount <= 0
            ? 0
            : (current + focusableCount - 1) % focusableCount,
        );
        return;
      }
      if (key.downArrow || normalizedInput === "j") {
        setFocusedIndex((current) =>
          focusableCount <= 0 ? 0 : (current + 1) % focusableCount,
        );
        return;
      }
      if (key.leftArrow || normalizedInput === "b") {
        sendControl(encodeSetupCommand("setup_prev_step"));
        return;
      }
      if (key.rightArrow || normalizedInput === "n") {
        sendControl(encodeSetupCommand("setup_next_step"));
        return;
      }

      if (setupState.step === "preview" && /^\d$/.test(input)) {
        const action = previewActions[Number(input) - 1];
        if (action) {
          executePreviewAction(action, sendControl);
        }
        return;
      }

      if (normalizedInput === "i" && setupState.step === "devices") {
        const device = setupState.available_devices[focusedIndex];
        if (device && selectedDeviceKeys.has(deviceIdentityKey(device.current))) {
          sendControl(
            encodeSetupCommand("setup_toggle_identify", {
              name: device.name,
            }),
          );
        }
        return;
      }

      if (setupState.step === "pairing") {
        const pairCount = setupState.config.pairings.length;
        const onNewPairRow = focusedIndex === pairCount;
        if (normalizedInput === "m") {
          // `m` always opens the picker. On an existing pair it edits
          // that pair (mutations apply immediately on the controller).
          // On the virtual `[+ new pair]` row it opens a *create* draft
          // — the operator's leader/follower selections are held UI-side
          // until BOTH are confirmed, at which point a single
          // `setup_create_pairing` lands the new pair on the controller.
          // Esc at any point during create-mode silently drops the draft,
          // so no half-baked pair is ever born.
          if (onNewPairRow) {
            setPairingDraft({
              kind: "create",
              phase: "leader",
              cursor: 0,
            });
            return;
          }
          const focusedPair = setupState.config.pairings[focusedIndex];
          if (focusedPair) {
            // Re-derive leader options against the focused pair so the
            // cursor lands on the pair's current leader (the memoized
            // `leaderOptions` still uses the previous picker context,
            // i.e. `undefined` here).
            const optionsForPair = eligibleLeaderOptions(setupState, focusedIndex);
            setPairingDraft({
              kind: "edit",
              index: focusedIndex,
              phase: "leader",
              cursor: Math.max(
                0,
                optionsForPair.findIndex(
                  (option) =>
                    option.deviceName === focusedPair.leader_device &&
                    option.channelType === focusedPair.leader_channel_type,
                ),
              ),
            });
          }
          return;
        }
        if (normalizedInput === "d" || key.delete || key.backspace) {
          // `d` only deletes existing pairs; on the virtual new-pair
          // row it's a no-op (there's nothing to delete yet).
          if (!onNewPairRow && setupState.config.pairings[focusedIndex]) {
            sendControl(
              encodeSetupCommand("setup_remove_pairing", { index: focusedIndex }),
            );
          }
          return;
        }
      }

      if (key.return) {
        if (setupState.step === "storage") {
          const field = settingsFields[focusedIndex];
          if (!field) {
            return;
          }
          if (field.kind === "text" && field.editableFieldId) {
            setEditingField(field.editableFieldId);
            setDraftValue(field.value);
          } else if (field.kind === "cycle" && field.action) {
            sendControl(encodeSetupCommand(field.action, { delta: 1 }));
          }
          return;
        }

        if (setupState.step === "preview") {
          const action = previewActions[focusedIndex];
          if (action) {
            executePreviewAction(action, sendControl);
          } else {
            sendControl(encodeSetupCommand("setup_save"));
          }
          return;
        }

        if (setupState.step === "devices") {
          const device = setupState.available_devices[focusedIndex];
          if (device && selectedDeviceKeys.has(deviceIdentityKey(device.current))) {
            const channel = device.current.channels[0];
            const initialName =
              channel?.name?.trim() ||
              channel?.channel_type ||
              device.current.name;
            setEditingField(deviceNameFieldId(device.name));
            setDraftValue(initialName);
          }
          return;
        }
        return;
      }

      if (input === " " && setupState.step === "devices") {
        const device = setupState.available_devices[focusedIndex];
        if (device) {
          sendControl(encodeSetupCommand("setup_toggle_device", { name: device.name }));
        }
        return;
      }

      if (setupState.step === "states") {
        const row = stateRows[focusedIndex];
        if (row && normalizedInput === "p") {
          sendControl(
            encodeSetupCommand("setup_toggle_publish_state", {
              name: row.deviceName,
              value: row.stateKind,
            }),
          );
          return;
        }
        if (row && normalizedInput === "e") {
          sendControl(
            encodeSetupCommand("setup_toggle_recorded_state", {
              name: row.deviceName,
              value: row.stateKind,
            }),
          );
          return;
        }
      }

      const delta =
        normalizedInput === "h"
          ? -1
          : normalizedInput === "l"
            ? 1
            : input === "["
              ? -1
              : input === "]"
                ? 1
                : 0;
      if (delta === 0) {
        return;
      }

      if (setupState.step === "devices") {
        const device = setupState.available_devices[focusedIndex];
        if (
          !device ||
          !selectedDeviceKeys.has(deviceIdentityKey(device.current))
        ) {
          return;
        }
        sendControl(
          encodeSetupCommand(
            device.device_type === "camera"
              ? "setup_cycle_camera_profile"
              : "setup_cycle_robot_mode",
            { name: device.name, delta },
          ),
        );
        return;
      }

      if (setupState.step === "pairing") {
        sendControl(
          encodeSetupCommand("setup_cycle_pair_mapping", {
            index: focusedIndex,
            delta,
          }),
        );
        return;
      }

      if (setupState.step === "storage") {
        const field = settingsFields[focusedIndex];
        if (field?.kind === "cycle" && field.action) {
          sendControl(encodeSetupCommand(field.action, { delta }));
        }
      }
    },
    {
      isActive: supportsInteractiveInput,
    },
  );

  return (
    <Box flexDirection="column" width={columns} height={rows}>
      <TitleBar
        mode="Setup"
        width={columns}
        wizardStep={{
          current: setupState?.step_index ?? 1,
          total: setupState?.total_steps ?? 1,
          name: setupState?.step_name ?? "Waiting",
        }}
      />

      {showLivePanels ? (
        <>
          {!previewConnected ? (
            <Box paddingX={1}>
              <Text color="yellow" bold>
                Launching preview...
              </Text>
            </Box>
          ) : null}
          <LivePreviewPanels
            key={livePanelsKey}
            frames={frames}
            robotChannels={robotChannels}
            streamInfo={streamInfo}
            connected={previewConnected}
            send={sendPreview}
            width={columns}
            availableRows={livePanelRows}
            cellGeometry={cellGeometry}
            rendererId={cameraRendererId}
            preferredCameraNames={preferredLiveCameraNames}
            hideEmptyRobotPanel={setupState?.step === "devices"}
          />
          <Box flexDirection="column" paddingX={1}>
            {detailLines.map(renderDetailLine)}
          </Box>
        </>
      ) : (
        <Box flexDirection="column" paddingX={1}>
          <Text bold color="cyan">
            {setupState ? `${setupState.step_name} Step` : "Waiting For Setup State"}
          </Text>
          {detailLines.map(renderDetailLine)}
        </Box>
      )}

      <KeyHintsBar hints={keyHints} width={columns} />
      <SetupStatusBar
        stepIndex={setupState?.step_index ?? 1}
        totalSteps={setupState?.total_steps ?? 1}
        connected={connected}
        outputPath={setupState?.output_path ?? "config.toml"}
        width={columns}
        status={setupState?.status ?? "editing"}
        message={setupState?.message}
      />
    </Box>
  );
}

function buildSettingsFields(setupState: SetupStateMessage | null): SettingsField[] {
  if (!setupState) {
    return [];
  }

  const storageTarget =
    setupState.config.storage.backend === "local"
      ? setupState.config.storage.output_path
      : (setupState.config.storage.endpoint ?? "");

  return [
    {
      id: "project_name",
      label: "Project name",
      value: setupState.config.project_name,
      kind: "text",
      editableFieldId: "project_name",
    },
    {
      id: "collection_mode",
      label: "Collection mode",
      value: setupState.config.mode,
      kind: "cycle",
      action: "setup_cycle_collection_mode",
    },
    {
      id: "episode_format",
      label: "Episode format",
      value: setupState.config.episode.format,
      kind: "cycle",
      action: "setup_cycle_episode_format",
    },
    {
      id: "video_codec",
      label: "RGB codec",
      value: formatCodecBackend(
        setupState.config.encoder.video_codec,
        setupState.config.encoder.video_backend ?? setupState.config.encoder.backend,
      ),
      kind: "cycle",
      action: "setup_cycle_video_codec",
    },
    {
      id: "depth_codec",
      label: "Depth codec",
      value: formatCodecBackend(
        setupState.config.encoder.depth_codec,
        setupState.config.encoder.depth_backend ?? setupState.config.encoder.backend,
      ),
      kind: "cycle",
      action: "setup_cycle_depth_codec",
    },
    {
      id: "storage_backend",
      label: "Storage backend",
      value: setupState.config.storage.backend,
      kind: "cycle",
      action: "setup_cycle_storage_backend",
    },
    {
      id:
        setupState.config.storage.backend === "local"
          ? "storage_output_path"
          : "storage_endpoint",
      label:
        setupState.config.storage.backend === "local"
          ? "Output path"
          : "HTTP endpoint",
      value: storageTarget,
      kind: "text",
      editableFieldId:
        setupState.config.storage.backend === "local"
          ? "storage_output_path"
          : "storage_endpoint",
    },
    {
      id: "ui_http_host",
      label: "UI host",
      value: setupState.config.ui?.http_host ?? "",
      kind: "text",
      editableFieldId: "ui_http_host",
    },
  ];
}

/** Render an `(EncoderCodec, EncoderBackend)` pair as a single human-readable
 *  label, e.g. `"av1 (nvidia)"`. The wizard cycles through this combined
 *  value so the operator can pick a specific encoder implementation in one
 *  step. The backend is omitted when unset (e.g. very old configs that only
 *  carried the legacy global `backend = auto`) so the label degrades to
 *  just the codec name. */
function formatCodecBackend(
  codec: string,
  backend: string | undefined,
): string {
  if (!backend) {
    return codec;
  }
  return `${codec} (${backend})`;
}

/** Flatten every selected robot channel's supported_states into a single
 *  list of toggleable rows. Camera channels and disabled robot channels
 *  are skipped because they have no toggleable states. */
function buildStateRows(setupState: SetupStateMessage | null): StateRow[] {
  if (!setupState) {
    return [];
  }
  const selectedKeys = new Set(
    setupState.config.devices.flatMap((device) =>
      device.channels
        .filter((channel) => channel.enabled !== false && channel.kind === "robot")
        .map((channel) => `${device.name}|${channel.channel_type}`),
    ),
  );
  const rows: StateRow[] = [];
  for (const available of setupState.available_devices) {
    if (available.device_type !== "robot") continue;
    const channel = primaryChannel(available.current);
    if (!channel) continue;
    const channelKey = `${available.current.name}|${channel.channel_type}`;
    if (!selectedKeys.has(channelKey)) continue;
    const channelLabel = `${available.current.name}/${channel.channel_type}`;
    const publishedSet = new Set(channel.publish_states ?? []);
    const recordedSet = new Set(channel.recorded_states ?? []);
    // Prefer the driver-reported supported_states so newly added kinds
    // surface even if the operator never configured them. Fall back to the
    // currently configured publish ∪ recorded set so the wizard still
    // lets the operator edit existing state lists when an older driver
    // (or a transient query) didn't expose supported_states.
    const advertised = available.supported_states ?? [];
    const candidates: string[] = [];
    const seen = new Set<string>();
    const pushUnique = (kind: string) => {
      if (seen.has(kind)) return;
      seen.add(kind);
      candidates.push(kind);
    };
    advertised.forEach(pushUnique);
    publishedSet.forEach(pushUnique);
    recordedSet.forEach(pushUnique);
    if (candidates.length === 0) continue;
    for (const stateKind of candidates) {
      rows.push({
        deviceName: available.name,
        channelLabel,
        stateKind,
        isPublished: publishedSet.has(stateKind),
        isRecorded: recordedSet.has(stateKind),
      });
    }
  }
  return rows;
}

/** Channel that satisfies the `supported_modes` predicate for one side of
 *  a teleop pair. Mirrors the controller's `eligible_leader_channels` /
 *  `eligible_follower_channels` so the picker only ever offers options
 *  the controller will accept. */
type PairChannelOption = {
  /** The `BinaryDeviceConfig.name` (== `bus_root`) of the device that
   *  owns the channel. Used as the pair endpoint identifier on the wire. */
  deviceName: string;
  channelType: string;
  /** Per-channel display name (falls back to channel_type), used to render
   *  the picker rows so the operator sees the same string they typed in
   *  the device step. */
  displayName: string;
  /** "{display}/{channel_type}" — used as a stable React key. */
  label: string;
};

function buildPairOptions(
  setupState: SetupStateMessage | null,
  predicate: (modes: SetupAvailableDevice["supported_modes"]) => boolean,
): PairChannelOption[] {
  if (!setupState) return [];
  const modesByKey = new Map<string, SetupAvailableDevice["supported_modes"]>();
  for (const available of setupState.available_devices) {
    if (available.device_type !== "robot") continue;
    const ch = available.current.channels[0];
    if (!ch) continue;
    modesByKey.set(
      `${available.driver}|${available.id}|${ch.channel_type}`,
      available.supported_modes,
    );
  }
  const out: PairChannelOption[] = [];
  for (const device of setupState.config.devices) {
    for (const channel of device.channels) {
      if (channel.kind !== "robot" || channel.enabled === false) {
        continue;
      }
      const supported =
        modesByKey.get(`${device.driver}|${device.id}|${channel.channel_type}`) ?? [];
      if (!predicate(supported)) continue;
      const displayName = (channel.name?.trim() || channel.channel_type) ?? channel.channel_type;
      out.push({
        deviceName: device.name,
        channelType: channel.channel_type,
        displayName,
        label: `${displayName}/${channel.channel_type}`,
      });
    }
  }
  return out;
}

/** Channels that may serve as the leader for the pair at
 *  `exceptPairIndex` (or for a brand-new pair when omitted). Mirrors the
 *  controller-side `SetupSession::eligible_leader_channels` so the picker
 *  never shows an option the controller would later reject. The leader
 *  must support `free-drive` OR `command-following` (either mode lets
 *  the controller observe joint state, which is all the leader needs to
 *  do); passive EEFs that only advertise free-drive (e.g. AIRBOT E2)
 *  qualify because the operator manually moves them and the controller
 *  forwards the observed positions. Leaders may be shared across pairs,
 *  so the only per-pair constraint is the no-self-loop guard against the
 *  targeted pair's current follower. */
function eligibleLeaderOptions(
  setupState: SetupStateMessage | null,
  exceptPairIndex?: number,
): PairChannelOption[] {
  const all = buildPairOptions(setupState, (modes) =>
    modes.includes("free-drive") || modes.includes("command-following"),
  );
  const targetedPair =
    exceptPairIndex !== undefined
      ? setupState?.config.pairings[exceptPairIndex]
      : undefined;
  return all.filter((option) => {
    if (
      targetedPair &&
      option.deviceName === targetedPair.follower_device &&
      option.channelType === targetedPair.follower_channel_type
    ) {
      return false;
    }
    return true;
  });
}

/** Channels that may serve as the follower for the pair at
 *  `exceptPairIndex` (or for a brand-new pair when omitted). Mirrors the
 *  controller-side `SetupSession::eligible_follower_channels`. The
 *  follower must support `command-following` and, additionally, must not
 *  already be a follower in another pair (each follower can only follow
 *  one leader at a time) and must not collapse onto its own pair's
 *  leader. */
function eligibleFollowerOptions(
  setupState: SetupStateMessage | null,
  exceptPairIndex?: number,
): PairChannelOption[] {
  const all = buildPairOptions(setupState, (modes) =>
    modes.includes("command-following"),
  );
  if (!setupState) return all;
  const targetedPair =
    exceptPairIndex !== undefined
      ? setupState.config.pairings[exceptPairIndex]
      : undefined;
  const claimedFollowers = new Set<string>();
  setupState.config.pairings.forEach((pair, idx) => {
    if (idx === exceptPairIndex) return;
    claimedFollowers.add(`${pair.follower_device}|${pair.follower_channel_type}`);
  });
  return all.filter((option) => {
    const key = `${option.deviceName}|${option.channelType}`;
    if (claimedFollowers.has(key)) return false;
    if (
      targetedPair &&
      option.deviceName === targetedPair.leader_device &&
      option.channelType === targetedPair.leader_channel_type
    ) {
      return false;
    }
    return true;
  });
}

function buildPreviewActions(setupState: SetupStateMessage | null): PreviewAction[] {
  if (!setupState) {
    return [];
  }

  const actions: PreviewAction[] = [
    { kind: "jump", label: "Edit devices", targetStep: "devices" },
    { kind: "jump", label: "Edit states", targetStep: "states" },
    { kind: "jump", label: "Edit settings", targetStep: "storage" },
  ];

  if (setupState.config.mode === "teleop" && setupState.config.pairings.length > 0) {
    actions.push({
      kind: "jump",
      label: "Edit pairings",
      targetStep: "pairing",
    });
  }

  actions.push({ kind: "save", label: "Save current config" });
  return actions;
}

function renderDetailLine(line: DetailLine) {
  return (
    <Text key={line.key}>
      {line.spans.map((span, index) => (
        <Text
          key={`${line.key}:${index}`}
          color={span.color}
          bold={span.bold}
          dimColor={span.dimColor}
        >
          {span.text}
        </Text>
      ))}
    </Text>
  );
}

function buildDetailLine(
  key: string,
  spans: Array<DetailSpan | null | false | undefined>,
): DetailLine {
  return {
    key,
    spans: spans.filter(
      (span): span is DetailSpan =>
        span != null && span !== false && span.text.length > 0,
    ),
  };
}

function textSegment(
  text: string,
  style: Omit<DetailSpan, "text"> = {},
): DetailSpan {
  return { text, ...style };
}

function textLine(
  key: string,
  text: string,
  style: Omit<DetailSpan, "text"> = {},
): DetailLine {
  return buildDetailLine(key, [textSegment(text, style)]);
}

function focusPrefix(focused: boolean, dimColor?: boolean): DetailSpan {
  return textSegment(`${focused ? ">" : " "} `, {
    color: focused ? "cyan" : undefined,
    bold: focused,
    dimColor,
  });
}

function noticeLine(
  key: string,
  label: string,
  message: string,
  color: string,
): DetailLine {
  return buildDetailLine(key, [
    textSegment(`${label}: `, { color, bold: true }),
    textSegment(message, { color }),
  ]);
}

function messageLine(
  message: string,
  status: SetupStateMessage["status"],
): DetailLine {
  const color =
    status === "saved"
      ? "green"
      : status === "cancelled" ||
          /(must not|already in use|requires|error|failed)/i.test(message)
        ? "yellow"
        : "cyan";
  return textLine("message", message, {
    color,
    bold: status !== "editing",
  });
}

function buildDetailLines(
  setupState: SetupStateMessage | null,
  focusedIndex: number,
  selectedDeviceKeys: Set<string>,
  settingsFields: SettingsField[],
  previewActions: PreviewAction[],
  editingField: EditableFieldId | null,
  draftValue: string,
  stateRows: StateRow[],
  pairingDraft: PairingDraft | null,
  leaderOptions: PairChannelOption[],
  followerOptions: PairChannelOption[],
): DetailLine[] {
  if (!setupState) {
    return [
      textLine(
        "waiting-state",
        "Waiting for the controller to publish setup state...",
        { color: "yellow", bold: true },
      ),
      textLine(
        "waiting-hint",
        "If this persists, confirm `rollio setup` launched the preview stack.",
        { color: "gray" },
      ),
    ];
  }

  const warningLines = setupState.warnings.map((warning, index) =>
    textLine(`warning:${index}`, `warning: ${warning}`, {
      color: "yellow",
      bold: true,
    }),
  );
  const messageLines = setupState.message
    ? [messageLine(setupState.message, setupState.status)]
    : [];

  switch (setupState.step) {
    case "devices": {
      const focusedDevice = setupState.available_devices[focusedIndex];
      const deviceRowWidths = computeDeviceRowWidths(
        setupState.available_devices,
      );
      return [
        textLine(
          "devices-title",
          "Select devices, set config names, and tune parameters before continuing.",
          { color: "cyan", bold: true },
        ),
        ...setupState.available_devices.map((device, index) =>
          deviceRowLine(
            device,
            index === focusedIndex,
            selectedDeviceKeys.has(deviceIdentityKey(device.current)),
            setupState.identify_device === device.name,
            editingField,
            draftValue,
            deviceRowWidths,
          ),
        ),
        ...(focusedDevice
          ? deviceDetails(
              focusedDevice,
              selectedDeviceKeys.has(deviceIdentityKey(focusedDevice.current)),
              setupState.identify_device === focusedDevice.name,
            )
          : []),
        ...warningLines,
        ...messageLines,
      ];
    }
    case "states": {
      if (stateRows.length === 0) {
        return [
          textLine(
            "states-empty",
            "No robot channels are selected. Go back and enable a robot channel first.",
            { color: "yellow", bold: true },
          ),
          ...warningLines,
          ...messageLines,
        ];
      }
      const stateKindWidth = stateRows.reduce(
        (acc, row) => Math.max(acc, row.stateKind.length),
        0,
      );
      let lastChannel: string | null = null;
      const lines: DetailLine[] = [
        textLine(
          "states-title",
          "Pick which states each robot channel publishes (P) and records (R).",
          { color: "cyan", bold: true },
        ),
      ];
      for (let index = 0; index < stateRows.length; index += 1) {
        const row = stateRows[index]!;
        if (row.channelLabel !== lastChannel) {
          lines.push(
            textLine(
              `states-channel:${row.channelLabel}`,
              row.channelLabel,
              { color: "magenta", bold: true },
            ),
          );
          lastChannel = row.channelLabel;
        }
        lines.push(stateRowLine(row, index === focusedIndex, stateKindWidth));
      }
      lines.push(...warningLines, ...messageLines);
      return lines;
    }
    case "pairing": {
      const pairCount = setupState.config.pairings.length;
      // The new-pair row sits at index = pairCount and is always
      // present, so the create flow stays discoverable even when the
      // pairing list is empty. `d` is intentionally a no-op on this row
      // (see m-key handler) — there's nothing to delete.
      const newPairFocused = focusedIndex === pairCount;
      const pairLines: DetailLine[] = [
        textLine(
          "pairing-title",
          "Manage teleoperation pairs. m: edit (or create on the new-pair row), d: delete.",
          { color: "cyan", bold: true },
        ),
        ...setupState.config.pairings.map((pair, index) =>
          buildDetailLine(`pair:${index}`, [
            focusPrefix(index === focusedIndex),
            textSegment(
              `${pair.leader_device}:${pair.leader_channel_type} -> ${pair.follower_device}:${pair.follower_channel_type}`,
              {
                bold: index === focusedIndex,
              },
            ),
            textSegment(` | ${pair.mapping}`, { color: "green" }),
          ]),
        ),
        buildDetailLine("pair:new", [
          focusPrefix(newPairFocused),
          textSegment("[+ new pair]", {
            bold: newPairFocused,
            color: "cyan",
          }),
          textSegment("  press m to create + edit", { color: "gray" }),
        ]),
      ];
      const pickerLines = pairingDraft
        ? buildPairingPickerLines(
            pairingDraft,
            pairingDraft.phase === "leader" ? leaderOptions : followerOptions,
          )
        : [];
      return [...pairLines, ...pickerLines, ...warningLines, ...messageLines];
    }
    case "storage": {
      const settingsLabelWidth = settingsFields.reduce(
        (acc, field) => Math.max(acc, field.label.length),
        0,
      );
      const settingsValueWidth = settingsFields.reduce((acc, field) => {
        const renderedValue = field.value || (field.kind === "text" ? "(empty)" : "");
        return Math.max(acc, renderedValue.length);
      }, 0);
      return [
        textLine(
          "storage-title",
          "Configure project metadata, collection mode, codecs, and storage target.",
          { color: "cyan", bold: true },
        ),
        ...settingsFields.map((field, index) =>
          settingsFieldLine(
            field,
            index === focusedIndex,
            editingField,
            draftValue,
            settingsLabelWidth,
            settingsValueWidth,
          ),
        ),
        ...warningLines,
        ...messageLines,
      ];
    }
    case "preview":
      return [
        buildDetailLine("preview-project", [
          textSegment("Project: ", { color: "cyan", bold: true }),
          textSegment(`${setupState.config.project_name} | Mode: ${setupState.config.mode}`),
        ]),
        buildDetailLine("preview-format", [
          textSegment("Format: ", { color: "cyan", bold: true }),
          textSegment(
            `${setupState.config.episode.format} | RGB: ${setupState.config.encoder.video_codec} | Depth: ${setupState.config.encoder.depth_codec}`,
          ),
        ]),
        buildDetailLine("preview-storage", [
          textSegment("Storage: ", { color: "cyan", bold: true }),
          textSegment(
            `${setupState.config.storage.backend} -> ${storageSummary(setupState)}`,
          ),
        ]),
        buildDetailLine("preview-counts", [
          textSegment("Devices: ", { color: "cyan", bold: true }),
          textSegment(
            `${setupState.config.devices.length} | Pairings: ${setupState.config.pairings.length}`,
          ),
        ]),
        ...previewActions.map((action, index) =>
          buildDetailLine(`preview-action:${index}`, [
            focusPrefix(index === focusedIndex),
            textSegment(`[${index + 1}] `, { color: "cyan" }),
            textSegment(action.label, {
              bold: index === focusedIndex,
              color: action.kind === "save" ? "green" : undefined,
            }),
          ]),
        ),
        ...messageLines,
        ...warningLines,
      ];
  }
}

function buildPairingPickerLines(
  draft: PairingDraft,
  options: PairChannelOption[],
): DetailLine[] {
  const phaseLabel = draft.phase === "leader" ? "leader" : "follower";
  const eligibilityHint =
    draft.phase === "leader"
      ? "channels that support free-drive or command-following"
      : "channels that support command-following";
  const headerColor = draft.phase === "leader" ? "magenta" : "cyan";
  const modeBadge =
    draft.kind === "create" ? "[new pair] " : "[edit pair] ";
  // In create mode's follower phase, surface the leader the operator
  // already locked in so they can confirm context before picking.
  const draftSummary =
    draft.kind === "create" && draft.draftLeader
      ? ` — leader: ${draft.draftLeader.label}`
      : "";
  if (options.length === 0) {
    return [
      buildDetailLine("pairing-picker-header", [
        textSegment(modeBadge, { color: headerColor, bold: true }),
        textSegment(`Pick ${phaseLabel}: `, { color: headerColor, bold: true }),
        textSegment(`(no eligible channels — ${eligibilityHint})`, { color: "yellow" }),
        draftSummary
          ? textSegment(draftSummary, { color: "gray" })
          : null,
      ].filter((span): span is DetailSpan => span !== null)),
      textLine(
        "pairing-picker-cancel",
        draft.kind === "create"
          ? "Press esc / m / q to drop the new-pair draft (nothing is saved); revisit step 1 to enable a compatible channel."
          : "Press esc / m / q to close the picker; revisit step 1 to enable a compatible channel.",
        { color: "gray" },
      ),
    ];
  }
  const lines: DetailLine[] = [
    buildDetailLine("pairing-picker-header", [
      textSegment(modeBadge, { color: headerColor, bold: true }),
      textSegment(`Pick ${phaseLabel} `, { color: headerColor, bold: true }),
      textSegment(`(${eligibilityHint})`, { color: "gray" }),
      draftSummary
        ? textSegment(draftSummary, { color: "gray" })
        : null,
      textSegment(
        "  [j/k: Move  enter: Select  esc / m / q: Cancel]",
        { color: "cyan" },
      ),
    ].filter((span): span is DetailSpan => span !== null)),
  ];
  options.forEach((option, index) => {
    const focused = index === draft.cursor;
    lines.push(
      buildDetailLine(`pairing-picker-option:${option.label}:${index}`, [
        focusPrefix(focused),
        textSegment(option.label, { bold: focused }),
      ]),
    );
  });
  return lines;
}

function stateRowLine(
  row: StateRow,
  focused: boolean,
  stateKindWidth: number,
): DetailLine {
  const publishGlyph = row.isPublished ? "P" : ".";
  const recordedGlyph = row.isRecorded ? "R" : ".";
  // Pad to the widest stateKind so the trailing "[p:Publish e:Record]"
  // hint lines up vertically across every row in the step.
  const paddedStateKind = row.stateKind.padEnd(stateKindWidth);
  return buildDetailLine(`state:${row.deviceName}:${row.stateKind}`, [
    focusPrefix(focused),
    textSegment(`[${publishGlyph} ${recordedGlyph}] `, {
      color: row.isPublished ? "green" : "gray",
      bold: focused,
    }),
    textSegment(paddedStateKind, {
      bold: focused,
      color: row.isPublished ? undefined : "gray",
    }),
    textSegment(" [p:Publish e:Record]", { color: "cyan" }),
  ]);
}

function settingsFieldLine(
  field: SettingsField,
  focused: boolean,
  editingField: EditableFieldId | null,
  draftValue: string,
  labelWidth: number,
  valueWidth: number,
): DetailLine {
  // Pad the label to the widest in the current set so every field's value
  // column starts at the same x-coordinate, no matter how long the label is.
  const paddedLabel = `${field.label}:`.padEnd(labelWidth + 1) + " ";
  if (field.kind === "cycle") {
    return buildDetailLine(`setting:${field.id}`, [
      focusPrefix(focused),
      textSegment(paddedLabel, { bold: true }),
      textSegment(field.value.padEnd(valueWidth), { color: "green" }),
      textSegment(" [h/l cycle]", { color: "cyan" }),
    ]);
  }

  const isEditing = field.editableFieldId === editingField;
  const rawValue = isEditing ? `${draftValue}|` : field.value || "(empty)";
  // Pad the value column too so the trailing `[Enter edit]` /
  // `[h/l cycle]` hints land at the same x-coordinate across rows.
  const displayValue = rawValue.padEnd(valueWidth);
  return buildDetailLine(`setting:${field.id}`, [
    focusPrefix(focused),
    textSegment(paddedLabel, { bold: true }),
    textSegment(displayValue, {
      color: field.value || isEditing ? undefined : "gray",
    }),
    textSegment(
      isEditing ? " [Enter save, Esc cancel]" : " [Enter edit]",
      { color: "cyan" },
    ),
  ]);
}

function executePreviewAction(
  action: PreviewAction,
  send: (message: string) => void,
) {
  if (action.kind === "save") {
    send(encodeSetupCommand("setup_save"));
    return;
  }
  send(
    encodeSetupCommand("setup_jump_step", {
      value: action.targetStep,
    }),
  );
}

function editCommandForField(
  field: StaticEditableFieldId,
): Extract<
  CommandAction,
  | "setup_set_project_name"
  | "setup_set_storage_output_path"
  | "setup_set_storage_endpoint"
  | "setup_set_ui_http_host"
> {
  switch (field) {
    case "project_name":
      return "setup_set_project_name";
    case "storage_output_path":
      return "setup_set_storage_output_path";
    case "storage_endpoint":
      return "setup_set_storage_endpoint";
    case "ui_http_host":
      return "setup_set_ui_http_host";
  }
}

function deviceNameFieldId(deviceKey: string): EditableFieldId {
  return `device_name:${deviceKey}`;
}

function deviceNameFieldKey(field: EditableFieldId): string | null {
  return field.startsWith("device_name:") ? field.slice("device_name:".length) : null;
}

function primaryChannel(
  device: SetupBinaryDeviceConfig,
): SetupDeviceChannelV2 | undefined {
  return device.channels[0];
}

function enabledChannelIdentityKeys(devices: SetupBinaryDeviceConfig[]): string[] {
  return devices.flatMap((device) =>
    device.channels
      .filter((channel) => channel.enabled !== false)
      .map((channel) =>
        [channel.kind ?? "camera", device.driver, device.id, channel.channel_type, "-"].join(
          "|",
        ),
      ),
  );
}

function enabledChannelNames(devices: SetupBinaryDeviceConfig[]): string[] {
  return devices.flatMap((device) =>
    device.channels
      .filter((channel) => channel.enabled !== false)
      .map((channel) => `${device.name}/${channel.channel_type}`),
  );
}

function enabledCameraNames(devices: SetupBinaryDeviceConfig[]): string[] {
  return devices.flatMap((device) =>
    device.channels
      .filter((channel) => channel.enabled !== false && channel.kind === "camera")
      .map((channel) => `${device.name}/${channel.channel_type}`),
  );
}

function configuredChannelSummary(device: SetupBinaryDeviceConfig): string {
  const channels = device.channels
    .filter((channel) => channel.enabled !== false)
    .map((channel) => channel.channel_type);
  return channels.length > 0 ? channels.join(",") : "(none)";
}

function deviceIdentityKey(device: SetupBinaryDeviceConfig): string {
  const ch = primaryChannel(device);
  const kind = ch?.kind ?? "camera";
  const channelType = ch?.channel_type ?? "-";
  return [kind, device.driver, device.id, channelType, "-"].join("|");
}

type DeviceRowWidths = {
  label: number;
  id: number;
  name: number;
  channels: number;
  config: number;
};

function deviceRowChannelName(device: SetupAvailableDevice): string {
  const channel = device.current.channels[0];
  return channel?.name?.trim() || channel?.channel_type || device.current.name;
}

function deviceRowLabel(device: SetupAvailableDevice): string {
  const channel = device.current.channels[0];
  return channel?.channel_label?.trim() || device.display_name;
}

function computeDeviceRowWidths(
  devices: SetupAvailableDevice[],
): DeviceRowWidths {
  let label = 0;
  let id = 0;
  let name = 0;
  let channels = 0;
  let config = 0;
  for (const device of devices) {
    label = Math.max(label, deviceRowLabel(device).length);
    id = Math.max(id, device.id.length);
    name = Math.max(name, deviceRowChannelName(device).length);
    channels = Math.max(
      channels,
      configuredChannelSummary(device.current).length,
    );
    config = Math.max(config, deviceConfigurationSummary(device).length);
  }
  return { label, id, name, channels, config };
}

function deviceRowLine(
  device: SetupAvailableDevice,
  focused: boolean,
  selected: boolean,
  identifying: boolean,
  editingField: EditableFieldId | null,
  draftValue: string,
  widths: DeviceRowWidths,
): DetailLine {
  const rowDim = !selected;
  const isEditing = editingField === deviceNameFieldId(device.name);
  // Per-channel name (with fallback to channel_type) — separate from the
  // parent BinaryDeviceConfig.name so renaming one row no longer mutates
  // sibling channels' rows.
  const channelName = deviceRowChannelName(device);
  const renderedName = isEditing ? `${draftValue}|` : channelName;
  // Per-channel display label (e.g. "AIRBOT E2") provided by the device
  // executable; fall back to device-level display_name when missing.
  const rowLabel = deviceRowLabel(device);
  const channelSummary = configuredChannelSummary(device.current);
  const configSummary = deviceConfigurationSummary(device);
  const dimStyle = {
    color: selected ? undefined : "gray",
    dimColor: rowDim,
  };
  return buildDetailLine(`device:${device.name}`, [
    focusPrefix(focused, rowDim),
    textSegment("[", { dimColor: rowDim }),
    textSegment(selected ? "x" : " ", {
      color: selected ? "green" : "gray",
      bold: selected,
    }),
    textSegment("] ", { dimColor: rowDim }),
    textSegment(rowLabel.padEnd(widths.label), {
      bold: focused || selected,
      ...dimStyle,
    }),
    textSegment(` | id=${device.id.padEnd(widths.id)}`, dimStyle),
    textSegment(` | name=${renderedName.padEnd(widths.name)}`, dimStyle),
    textSegment(
      ` | channels=${channelSummary.padEnd(widths.channels)}`,
      dimStyle,
    ),
    textSegment(` | ${configSummary.padEnd(widths.config)}`, dimStyle),
    identifying
      ? textSegment(" [identifying]", { color: "yellow", bold: true })
      : null,
    isEditing
      ? textSegment(" [Enter save, Esc cancel]", { color: "cyan" })
      : null,
  ]);
}

function deviceDetails(
  device: SetupAvailableDevice,
  selected: boolean,
  identifying: boolean,
): DetailLine[] {
  const channelSummary = configuredChannelSummary(device.current);
  if (device.device_type === "camera") {
    const ch = primaryChannel(device.current);
    const transport = extraString(device.current.extra, "transport");
    const iface = extraString(device.current.extra, "interface");
    return [
      buildDetailLine("focused-camera", [
        textSegment("Focused camera: ", { color: "cyan", bold: true }),
        textSegment(
          [
            `driver=${device.driver}`,
            `channels=${channelSummary}`,
            `channel=${ch?.channel_type ?? "default"}`,
            `pixel=${cameraProfileFormatLabel(ch?.profile)}`,
            transport ? `transport=${transport}` : null,
            iface ? `interface=${iface}` : null,
          ]
            .filter(Boolean)
            .join(" | "),
        ),
      ]),
      selected
        ? identifying
          ? noticeLine(
              "camera-identify-active",
              "Identify active",
              "Live preview is shown below for the focused selected camera.",
              "yellow",
            )
          : noticeLine(
              "camera-identify-hint",
              "Identify",
              "Press i to launch a live preview for the focused selected camera.",
              "cyan",
            )
        : null,
    ].filter((line): line is DetailLine => line !== null);
  }

  const transport = extraString(device.current.extra, "transport");
  const iface = extraString(device.current.extra, "interface");
  const productVariant = extraString(device.current.extra, "product_variant");
  const endEffector = extraString(device.current.extra, "end_effector");
  const robotIdentity = [
    `driver=${device.driver}`,
    `id=${device.id}`,
    `channels=${channelSummary}`,
    iface ? `interface=${iface}` : null,
    transport ? `transport=${transport}` : null,
    productVariant ? `variant=${productVariant}` : null,
    endEffector ? `eef=${endEffector}` : null,
  ]
    .filter(Boolean)
    .join(" | ");
  return [
    buildDetailLine("focused-robot", [
      textSegment("Focused robot: ", { color: "cyan", bold: true }),
      textSegment(robotIdentity),
    ]),
    selected
      ? identifying
        ? noticeLine(
            "robot-identify-active",
            "Identify active",
            "The focused robot channel is running in identifying mode.",
            "yellow",
          )
        : noticeLine(
            "robot-identify-hint",
            "Identify",
            "Press i to switch the focused selected robot channel into identifying mode.",
            "cyan",
          )
      : null,
  ].filter((line): line is DetailLine => line !== null);
}

function extraString(extra: Record<string, unknown> | undefined, key: string): string | null {
  const v = extra?.[key];
  return typeof v === "string" ? v : null;
}

function cameraProfileFormatLabel(
  profile:
    | { pixel_format?: string | null; native_pixel_format?: string | null }
    | null
    | undefined,
): string {
  const outputFormat = profile?.pixel_format ?? "unknown";
  const nativeFormat = profile?.native_pixel_format ?? null;
  return nativeFormat && nativeFormat.toLowerCase() !== outputFormat.toLowerCase()
    ? `${nativeFormat}->${outputFormat}`
    : outputFormat;
}

function deviceConfigurationSummary(device: SetupAvailableDevice): string {
  const ch = primaryChannel(device.current);
  if (device.device_type === "camera") {
    const p = ch?.profile;
    return `${p?.width ?? "?"}x${p?.height ?? "?"} ${p?.fps ?? "?"}fps ${cameraProfileFormatLabel(p)}`;
  }
  const controlRate =
    ch?.control_frequency_hz != null
      ? `${ch.control_frequency_hz}Hz`
      : "driver default";
  return `${ch?.mode ?? "free-drive"} @ ${controlRate}`;
}

function storageSummary(setupState: SetupStateMessage): string {
  return setupState.config.storage.backend === "local"
    ? setupState.config.storage.output_path
    : (setupState.config.storage.endpoint ?? "(unset)");
}

type BuildSetupKeyHintsArgs = {
  setupState: SetupStateMessage | null;
  editingField: EditableFieldId | null;
  pairingDraft: PairingDraft | null;
  previewActionCount: number;
  showLivePanels: boolean;
  rendererLabel: string;
};

/** Build the per-step key hint list shown by `KeyHintsBar`. The fixed
 *  navigation keys (`b/n`, `q`) sit at the end of each list so the
 *  operator-facing step verbs stay at the top. `r:Renderer [<label>]`
 *  is only included when a live preview is on screen. */
function buildSetupKeyHints({
  setupState,
  editingField,
  pairingDraft,
  previewActionCount,
  showLivePanels,
  rendererLabel,
}: BuildSetupKeyHintsArgs): KeyHint[] {
  if (editingField !== null) {
    return [
      { key: "type", label: "Edit text" },
      { key: "enter", label: "Save" },
      { key: "esc", label: "Cancel" },
    ];
  }

  if (pairingDraft !== null && setupState?.step === "pairing") {
    return [
      { key: "j/k", label: "Move" },
      { key: "enter", label: "Select" },
      // `m` and `q` are accepted as cancel-fallbacks because some
      // terminals delay the bare escape byte that drives `key.escape`.
      { key: "esc / m / q", label: "Cancel" },
    ];
  }

  const navTail: KeyHint[] = [
    { key: "b", label: "Previous Step" },
    { key: "n", label: "Next Step" },
    ...(showLivePanels
      ? ([{ key: "r", label: `Renderer [${rendererLabel}]` }] as KeyHint[])
      : []),
    { key: "q", label: "Cancel" },
  ];

  switch (setupState?.step) {
    case "devices":
      return [
        { key: "j/k", label: "Switch Focus" },
        { key: "space", label: "Toggle Select" },
        { key: "enter", label: "Rename" },
        { key: "[/]", label: "Switch Profile" },
        { key: "i", label: "Identify" },
        ...navTail,
      ];
    case "states":
      return [
        { key: "j/k", label: "Switch Focus" },
        { key: "p", label: "Toggle Publish" },
        { key: "e", label: "Toggle Record" },
        ...navTail,
      ];
    case "pairing":
      return [
        { key: "j/k", label: "Switch Focus" },
        { key: "m", label: "New / Edit Pair" },
        { key: "d", label: "Delete Pair" },
        { key: "[/]", label: "Cycle Mapping" },
        ...navTail,
      ];
    case "storage":
      return [
        { key: "j/k", label: "Switch Focus" },
        { key: "enter", label: "Edit / Cycle" },
        { key: "[/]", label: "Cycle" },
        ...navTail,
      ];
    case "preview":
      return [
        { key: "j/k", label: "Switch Focus" },
        { key: "enter", label: previewActionCount > 0 ? "Select" : "Save" },
        ...(previewActionCount > 0
          ? ([{ key: "1-9", label: "Jump" }] as KeyHint[])
          : []),
        ...navTail,
      ];
    default:
      return navTail;
  }
}
