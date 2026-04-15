import {
  defaultAsciiRendererId,
  isAsciiRendererId,
  type AsciiRendererId,
} from "./lib/renderers/index.js";

export type EpisodeKeyBindings = {
  startKey: string;
  stopKey: string;
  keepKey: string;
  discardKey: string;
};

export type AppMode = "collect" | "setup";

export type UiRuntimeConfig = {
  appMode: AppMode;
  websocketUrl: string;
  asciiRendererId: AsciiRendererId;
  episodeKeyBindings: EpisodeKeyBindings;
};

const DEFAULT_WS_URL = "ws://localhost:9090";
const DEFAULT_APP_MODE: AppMode = "collect";
const DEFAULT_START_KEY = "s";
const DEFAULT_STOP_KEY = "e";
const DEFAULT_KEEP_KEY = "k";
const DEFAULT_DISCARD_KEY = "x";

function takeFlagValue(argv: string[], flagNames: string[]): string | undefined {
  for (let idx = 0; idx < argv.length; idx += 1) {
    const arg = argv[idx];
    for (const flagName of flagNames) {
      if (arg === flagName) {
        return argv[idx + 1];
      }
      if (arg.startsWith(`${flagName}=`)) {
        return arg.slice(flagName.length + 1);
      }
    }
  }

  return undefined;
}

export function resolveRuntimeConfig(
  argv: string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): UiRuntimeConfig {
  const cliMode = takeFlagValue(argv, ["--mode"]);
  const cliWsUrl = takeFlagValue(argv, ["--ws", "--websocket-url"]);
  const cliRenderer = takeFlagValue(argv, ["--renderer", "--ascii-renderer"]);
  const cliStartKey = takeFlagValue(argv, ["--start-key"]);
  const cliStopKey = takeFlagValue(argv, ["--stop-key"]);
  const cliKeepKey = takeFlagValue(argv, ["--keep-key"]);
  const cliDiscardKey = takeFlagValue(argv, ["--discard-key"]);
  const selectedMode = cliMode?.trim().toLowerCase() || env.ROLLIO_UI_MODE?.trim().toLowerCase();
  const appMode: AppMode = selectedMode === "setup" ? "setup" : DEFAULT_APP_MODE;
  const envWsUrl = env.ROLLIO_VISUALIZER_WS ?? env.ROLLIO_UI_WS;
  const websocketUrl = cliWsUrl?.trim() || envWsUrl?.trim() || DEFAULT_WS_URL;
  const selectedRenderer = cliRenderer?.trim() || env.ROLLIO_ASCII_RENDERER?.trim();
  const asciiRendererId =
    selectedRenderer && isAsciiRendererId(selectedRenderer)
      ? selectedRenderer
      : defaultAsciiRendererId();
  const episodeKeyBindings = {
    startKey:
      cliStartKey?.trim().toLowerCase() ||
      env.ROLLIO_UI_START_KEY?.trim().toLowerCase() ||
      DEFAULT_START_KEY,
    stopKey:
      cliStopKey?.trim().toLowerCase() ||
      env.ROLLIO_UI_STOP_KEY?.trim().toLowerCase() ||
      DEFAULT_STOP_KEY,
    keepKey:
      cliKeepKey?.trim().toLowerCase() ||
      env.ROLLIO_UI_KEEP_KEY?.trim().toLowerCase() ||
      DEFAULT_KEEP_KEY,
    discardKey:
      cliDiscardKey?.trim().toLowerCase() ||
      env.ROLLIO_UI_DISCARD_KEY?.trim().toLowerCase() ||
      DEFAULT_DISCARD_KEY,
  };

  return { appMode, websocketUrl, asciiRendererId, episodeKeyBindings };
}
