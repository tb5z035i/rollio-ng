import React, { useEffect, useMemo, useState } from "react";
import { Box, Text, useInput, useStdin, useStdout } from "ink";
import { TitleBar } from "./components/TitleBar.js";
import { SetupStatusBar } from "./components/SetupStatusBar.js";
import { LivePreviewPanels } from "./components/LivePreviewPanels.js";
import { useWebSocket } from "./lib/websocket.js";
import {
  encodeSetupCommand,
  type CommandAction,
  type SetupAvailableDevice,
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
  | "storage_endpoint";
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

type PreviewJumpStep = "devices" | "storage" | "pairing";

type PreviewAction =
  | { kind: "jump"; label: string; targetStep: PreviewJumpStep }
  | { kind: "save"; label: string };

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
  websocketUrl: string;
  initialAsciiRendererId: AsciiRendererId;
};

export function SetupApp({
  websocketUrl,
  initialAsciiRendererId,
}: SetupAppProps) {
  const { columns, rows, cellGeometry } = useTerminalMetrics();
  const { isRawModeSupported } = useStdin();
  const supportsInteractiveInput = isRawModeSupported === true;
  const {
    frames,
    robotStates,
    streamInfo,
    connected,
    send,
    setupState,
  } = useWebSocket(websocketUrl);
  const [cameraRendererId, setCameraRendererId] = useState<AsciiRendererId>(
    initialAsciiRendererId,
  );
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [editingField, setEditingField] = useState<EditableFieldId | null>(null);
  const [draftValue, setDraftValue] = useState("");

  useEffect(() => {
    if (!connected) {
      return;
    }
    send(encodeSetupCommand("setup_get_state"));
    const interval = setInterval(() => {
      send(encodeSetupCommand("setup_get_state"));
    }, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [connected, send]);

  const selectedDevices = useMemo(
    () => setupState?.config.devices ?? [],
    [setupState],
  );
  const selectedDeviceKeys = useMemo(
    () => new Set(selectedDevices.map((device) => deviceIdentityKey(device))),
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
  const focusableCount = useMemo(() => {
    switch (setupState?.step) {
      case "devices":
        return setupState.available_devices.length;
      case "pairing":
        return setupState.config.pairing.length;
      case "storage":
        return settingsFields.length;
      case "preview":
        return previewActions.length;
      default:
        return 0;
    }
  }, [previewActions.length, settingsFields.length, setupState]);

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
      ),
    [
      draftValue,
      editingField,
      focusedIndex,
      selectedDeviceKeys,
      previewActions,
      settingsFields,
      setupState,
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
    return `preview:${selectedDevices.map((device) => device.name).join("|")}`;
  }, [selectedDevices, setupState]);
  const livePanelRows = useMemo(() => {
    if (!showLivePanels) {
      return 0;
    }
    return Math.max(8, rows - 2 - detailLines.length - 1);
  }, [detailLines.length, rows, showLivePanels]);
  const preferredLiveCameraNames = useMemo(() => {
    if (!setupState) {
      return [];
    }
    if (setupState.step === "preview") {
      return selectedDevices
        .filter((device) => device.type === "camera")
        .map((device) => device.name);
    }
    if (
      setupState.step === "devices" &&
      identifyDevice?.device_type === "camera"
    ) {
      return [identifyDevice.current.name];
    }
    return [];
  }, [identifyDevice, selectedDevices, setupState]);
  const stepHint = useMemo(
    () => stepHintForState(setupState, editingField, previewActions.length),
    [editingField, previewActions.length, setupState],
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
            send(
              encodeSetupCommand("setup_set_device_name", {
                name: deviceNameKey,
                value: draftValue,
              }),
            );
          } else {
            send(
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

      const normalizedInput = input.toLowerCase();
      if (normalizedInput === "r") {
        setCameraRendererId((current) => nextAsciiRendererId(current));
        return;
      }
      if (normalizedInput === "q") {
        send(encodeSetupCommand("setup_cancel"));
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
        send(encodeSetupCommand("setup_prev_step"));
        return;
      }
      if (key.rightArrow || normalizedInput === "n") {
        send(encodeSetupCommand("setup_next_step"));
        return;
      }

      if (setupState.step === "preview" && /^\d$/.test(input)) {
        const action = previewActions[Number(input) - 1];
        if (action) {
          executePreviewAction(action, send);
        }
        return;
      }

      if (normalizedInput === "i" && setupState.step === "devices") {
        const device = setupState.available_devices[focusedIndex];
        if (device && selectedDeviceKeys.has(deviceIdentityKey(device.current))) {
          send(
            encodeSetupCommand("setup_toggle_identify", {
              name: device.name,
            }),
          );
        }
        return;
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
            send(encodeSetupCommand(field.action, { delta: 1 }));
          }
          return;
        }

        if (setupState.step === "preview") {
          const action = previewActions[focusedIndex];
          if (action) {
            executePreviewAction(action, send);
          } else {
            send(encodeSetupCommand("setup_save"));
          }
          return;
        }

        if (setupState.step === "devices") {
          const device = setupState.available_devices[focusedIndex];
          if (device && selectedDeviceKeys.has(deviceIdentityKey(device.current))) {
            setEditingField(deviceNameFieldId(device.name));
            setDraftValue(device.current.name);
          }
          return;
        }
        return;
      }

      if (input === " " && setupState.step === "devices") {
        const device = setupState.available_devices[focusedIndex];
        if (device) {
          send(encodeSetupCommand("setup_toggle_device", { name: device.name }));
        }
        return;
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
        send(
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
        send(
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
          send(encodeSetupCommand(field.action, { delta }));
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
          <LivePreviewPanels
            key={livePanelsKey}
            frames={frames}
            robotStates={robotStates}
            streamInfo={streamInfo}
            connected={connected}
            send={send}
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

      <SetupStatusBar
        stepIndex={setupState?.step_index ?? 1}
        totalSteps={setupState?.total_steps ?? 1}
        connected={connected}
        outputPath={setupState?.output_path ?? "config.toml"}
        width={columns}
        status={setupState?.status ?? "editing"}
        stepHint={`${stepHint} | r:Renderer ${rendererLabel}`}
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
      value: setupState.config.encoder.video_codec,
      kind: "cycle",
      action: "setup_cycle_video_codec",
    },
    {
      id: "depth_codec",
      label: "Depth codec",
      value: setupState.config.encoder.depth_codec,
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
  ];
}

function buildPreviewActions(setupState: SetupStateMessage | null): PreviewAction[] {
  if (!setupState) {
    return [];
  }

  const actions: PreviewAction[] = [
    { kind: "jump", label: "Edit devices", targetStep: "devices" },
    { kind: "jump", label: "Edit settings", targetStep: "storage" },
  ];

  if (setupState.config.mode === "teleop" && setupState.config.pairing.length > 0) {
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
    case "pairing":
      return setupState.config.pairing.length > 0
        ? [
            textLine(
              "pairing-title",
              "Review teleoperation mappings for leader/follower pairs.",
              { color: "cyan", bold: true },
            ),
            ...setupState.config.pairing.map((pair, index) =>
              buildDetailLine(`pair:${index}`, [
                focusPrefix(index === focusedIndex),
                textSegment(`${pair.leader} -> ${pair.follower}`, {
                  bold: index === focusedIndex,
                }),
                textSegment(` | ${pair.mapping}`, { color: "green" }),
              ]),
            ),
            ...warningLines,
            ...messageLines,
          ]
        : [
            textLine("pairing-empty", "No teleop pairings are active.", {
              color: "yellow",
              bold: true,
            }),
            textLine(
              "pairing-hint",
              "Switch collection mode to teleop from Settings to enable pair editing.",
              { color: "gray" },
            ),
            ...warningLines,
            ...messageLines,
          ];
    case "storage":
      return [
        textLine(
          "storage-title",
          "Configure project metadata, collection mode, codecs, and storage target.",
          { color: "cyan", bold: true },
        ),
        ...settingsFields.map((field, index) =>
          settingsFieldLine(field, index === focusedIndex, editingField, draftValue),
        ),
        ...warningLines,
        ...messageLines,
      ];
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
            `${setupState.config.devices.length} | Pairings: ${setupState.config.pairing.length}`,
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

function settingsFieldLine(
  field: SettingsField,
  focused: boolean,
  editingField: EditableFieldId | null,
  draftValue: string,
): DetailLine {
  if (field.kind === "cycle") {
    return buildDetailLine(`setting:${field.id}`, [
      focusPrefix(focused),
      textSegment(`${field.label}: `, { bold: true }),
      textSegment(field.value, { color: "green" }),
      textSegment(" [h/l cycle]", { color: "cyan" }),
    ]);
  }

  const isEditing = field.editableFieldId === editingField;
  const displayValue = isEditing ? `${draftValue}|` : field.value || "(empty)";
  return buildDetailLine(`setting:${field.id}`, [
    focusPrefix(focused),
    textSegment(`${field.label}: `, { bold: true }),
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
> {
  switch (field) {
    case "project_name":
      return "setup_set_project_name";
    case "storage_output_path":
      return "setup_set_storage_output_path";
    case "storage_endpoint":
      return "setup_set_storage_endpoint";
  }
}

function deviceNameFieldId(deviceKey: string): EditableFieldId {
  return `device_name:${deviceKey}`;
}

function deviceNameFieldKey(field: EditableFieldId): string | null {
  return field.startsWith("device_name:") ? field.slice("device_name:".length) : null;
}

function deviceIdentityKey(
  device: Pick<
    SetupAvailableDevice["current"],
    "type" | "driver" | "id" | "stream" | "channel"
  >,
): string {
  return [
    device.type,
    device.driver,
    device.id,
    device.stream ?? "-",
    device.channel ?? "-",
  ].join("|");
}

function deviceRowLine(
  device: SetupAvailableDevice,
  focused: boolean,
  selected: boolean,
  identifying: boolean,
  editingField: EditableFieldId | null,
  draftValue: string,
): DetailLine {
  const rowDim = !selected;
  const isEditing = editingField === deviceNameFieldId(device.name);
  const renderedName = isEditing ? `${draftValue}|` : device.current.name;
  return buildDetailLine(`device:${device.name}`, [
    focusPrefix(focused, rowDim),
    textSegment("[", { dimColor: rowDim }),
    textSegment(selected ? "x" : " ", {
      color: selected ? "green" : "gray",
      bold: selected,
    }),
    textSegment("] ", { dimColor: rowDim }),
    textSegment(device.display_name, {
      bold: focused || selected,
      color: selected ? undefined : "gray",
      dimColor: rowDim,
    }),
    identifying
      ? textSegment(" [identifying]", { color: "yellow", bold: true })
      : null,
    textSegment(` | id=${device.id}`, {
      color: selected ? undefined : "gray",
      dimColor: rowDim,
    }),
    textSegment(" | name=", {
      color: selected ? undefined : "gray",
      dimColor: rowDim,
    }),
    textSegment(renderedName, {
      color: selected ? undefined : "gray",
      dimColor: rowDim,
    }),
    isEditing
      ? textSegment(" [Enter save, Esc cancel]", { color: "cyan" })
      : null,
    textSegment(` | ${deviceConfigurationSummary(device)}`, {
      color: selected ? undefined : "gray",
      dimColor: rowDim,
    }),
  ]);
}

function deviceDetails(
  device: SetupAvailableDevice,
  selected: boolean,
  identifying: boolean,
): DetailLine[] {
  if (device.device_type === "camera") {
    return [
      buildDetailLine("focused-camera", [
        textSegment("Focused camera: ", { color: "cyan", bold: true }),
        textSegment(
          [
            `driver=${device.driver}`,
            `stream=${device.current.stream ?? "default"}`,
            `pixel=${device.current.pixel_format ?? "unknown"}`,
            device.current.transport
              ? `transport=${device.current.transport}`
              : null,
            device.current.interface
              ? `interface=${device.current.interface}`
              : null,
          ]
            .filter(Boolean)
            .join(" | "),
        ),
      ]),
      selected
        ? noticeLine(
            "camera-selected",
            "Selected",
            "Space toggles, Enter renames, h/l or [/] cycles camera profiles.",
            "green",
          )
        : noticeLine(
            "camera-inactive",
            "Inactive",
            "Press Space to select this camera before renaming, tuning, or identify.",
            "yellow",
          ),
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
        : noticeLine(
            "camera-identify-disabled",
            "Identify locked",
            "Identify is only available for selected devices.",
            "gray",
          ),
    ];
  }

  const isAirbot = device.driver.startsWith("airbot-");
  const robotIdentity = [
    `driver=${device.driver}`,
    isAirbot ? `sn=${device.id}` : `id=${device.id}`,
    `interface=${device.current.interface ?? "n/a"}`,
    `transport=${device.current.transport ?? "n/a"}`,
    device.current.product_variant
      ? `variant=${device.current.product_variant}`
      : null,
    device.current.end_effector ? `eef=${device.current.end_effector}` : null,
  ]
    .filter(Boolean)
    .join(" | ");
  return [
    buildDetailLine("focused-robot", [
      textSegment("Focused robot: ", { color: "cyan", bold: true }),
      textSegment(robotIdentity),
    ]),
    selected
      ? noticeLine(
          "robot-selected",
          "Selected",
          "Space toggles, Enter renames, h/l or [/] cycles robot modes.",
          "green",
        )
      : noticeLine(
          "robot-inactive",
          "Inactive",
          "Press Space to select this robot before renaming, tuning, or identify.",
          "yellow",
        ),
    selected
      ? identifying
        ? isAirbot
          ? noticeLine(
              "robot-identify-active-airbot",
              "Identify active",
              "AIRBOT is in free-drive with the LED blinking orange.",
              "yellow",
            )
          : noticeLine(
              "robot-identify-active",
              "Identify active",
              "Live robot state is shown below.",
              "yellow",
            )
        : isAirbot
          ? noticeLine(
              "robot-identify-hint-airbot",
              "Identify",
              "Press i to enter free-drive and blink the AIRBOT LED orange.",
              "cyan",
            )
          : noticeLine(
              "robot-identify-hint",
              "Identify",
              "Press i to start identify for the focused selected robot.",
              "cyan",
            )
      : noticeLine(
          "robot-identify-disabled",
          "Identify locked",
          "Identify is only available for selected devices.",
          "gray",
        ),
  ];
}

function deviceConfigurationSummary(device: SetupAvailableDevice): string {
  if (device.device_type === "camera") {
    const current = device.current;
    return `${current.width ?? "?"}x${current.height ?? "?"} ${current.fps ?? "?"}fps ${current.pixel_format ?? "unknown"}`;
  }
  const controlRate =
    device.current.control_frequency_hz != null
      ? `${device.current.control_frequency_hz}Hz`
      : "driver default";
  return `${device.current.mode ?? "free-drive"} @ ${controlRate}`;
}

function storageSummary(setupState: SetupStateMessage): string {
  return setupState.config.storage.backend === "local"
    ? setupState.config.storage.output_path
    : (setupState.config.storage.endpoint ?? "(unset)");
}

function stepHintForState(
  setupState: SetupStateMessage | null,
  editingField: EditableFieldId | null,
  previewActionCount: number,
): string {
  if (editingField !== null) {
    return "Type text Enter:Save Esc:Cancel";
  }

  switch (setupState?.step) {
    case "devices":
      return "j/k:Focus space:Toggle Enter:Rename h/l or [/] Cycle i:Identify b/n:Step q:Cancel";
    case "pairing":
      return "j/k:Focus h/l Cycle mapping b/n:Step q:Cancel";
    case "storage":
      return "j/k:Field Enter:Edit h/l or [/] Cycle b/n:Step q:Cancel";
    case "preview":
      return previewActionCount > 0
        ? "j/k:Action Enter:Select 1-9:Jump b:Back q:Cancel"
        : "Enter:Save b:Back q:Cancel";
    default:
      return "b/n:Step q:Cancel";
  }
}
