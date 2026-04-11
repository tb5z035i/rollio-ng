import type { EpisodeKeyBindings } from "../runtime-config.js";
import type { CommandAction } from "./protocol.js";

type EpisodeCommandAction = Extract<
  CommandAction,
  "episode_start" | "episode_stop" | "episode_keep" | "episode_discard"
>;

export type UiInputAction =
  | "toggle_debug"
  | "cycle_renderer"
  | EpisodeCommandAction;

export function actionForInput(
  input: string,
  episodeKeyBindings: EpisodeKeyBindings,
): UiInputAction | null {
  const normalized = input.toLowerCase();
  if (normalized === "d") {
    return "toggle_debug";
  }
  if (normalized === "r") {
    return "cycle_renderer";
  }
  if (normalized === episodeKeyBindings.startKey) {
    return "episode_start";
  }
  if (normalized === episodeKeyBindings.stopKey) {
    return "episode_stop";
  }
  if (normalized === episodeKeyBindings.keepKey) {
    return "episode_keep";
  }
  if (normalized === episodeKeyBindings.discardKey) {
    return "episode_discard";
  }
  return null;
}
