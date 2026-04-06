import {
  defaultAsciiRendererId,
  isAsciiRendererId,
  type AsciiRendererId,
} from "./lib/renderers/index.js";

export type UiRuntimeConfig = {
  websocketUrl: string;
  asciiRendererId: AsciiRendererId;
};

const DEFAULT_WS_URL = "ws://localhost:9090";

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
  const cliWsUrl = takeFlagValue(argv, ["--ws", "--websocket-url"]);
  const cliRenderer = takeFlagValue(argv, ["--renderer", "--ascii-renderer"]);
  const envWsUrl = env.ROLLIO_VISUALIZER_WS ?? env.ROLLIO_UI_WS;
  const websocketUrl = cliWsUrl?.trim() || envWsUrl?.trim() || DEFAULT_WS_URL;
  const selectedRenderer = cliRenderer?.trim() || env.ROLLIO_ASCII_RENDERER?.trim();
  const asciiRendererId =
    selectedRenderer && isAsciiRendererId(selectedRenderer)
      ? selectedRenderer
      : defaultAsciiRendererId();

  return { websocketUrl, asciiRendererId };
}
