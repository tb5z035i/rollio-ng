import React from "react";
import { Box, Text } from "ink";

/**
 * One key->action hint shown in the bottom keys row. `key` is the literal
 * keystroke (e.g. `j/k`, `space`, `enter`, `[/]`); `label` is the
 * operator-facing action description (e.g. `Switch Focus`).
 *
 * `color` is optional; when omitted, the bar assigns one from a fixed
 * rotating palette so each action is visually distinct without callers
 * having to think about color choice.
 */
export interface KeyHint {
  key: string;
  label: string;
  color?: string;
}

interface KeyHintsBarProps {
  hints: KeyHint[];
  width: number;
  /** Optional secondary doc/help row rendered below the keys row. */
  docRow?: string;
}

/** Color palette used to differentiate consecutive key hints when callers
 *  don't pin a specific color. Picked to read well against the default
 *  ink/terminal background and to stay visually distinct between adjacent
 *  hints. */
const HINT_COLOR_PALETTE = [
  "cyan",
  "green",
  "yellow",
  "magenta",
  "blue",
  "red",
  "white",
] as const;

function colorForIndex(index: number, override?: string): string {
  return override ?? HINT_COLOR_PALETTE[index % HINT_COLOR_PALETTE.length]!;
}

/** A two-line bar (keys row + optional doc row) rendered above the status
 *  bar. Replaces the old single-line `Keys: ...` segment that used to be
 *  squashed into the status bar's left half. The row is prefixed with a
 *  bold + underlined "Key hints:" anchor so the operator's eye lands on
 *  it without scanning the full width. */
export function KeyHintsBar({ hints, width, docRow }: KeyHintsBarProps) {
  return (
    <Box flexDirection="column" width={width} paddingX={1}>
      <Box>
        <Text>
          <Text bold underline>
            Key hints:
          </Text>
          <Text>{" "}</Text>
          {hints.map((hint, index) => {
            const color = colorForIndex(index, hint.color);
            return (
              <Text key={`${hint.key}:${index}`}>
                {index > 0 ? <Text dimColor>{"  "}</Text> : null}
                <Text color={color} bold>
                  {hint.key}
                </Text>
                <Text>{`: ${hint.label}`}</Text>
              </Text>
            );
          })}
        </Text>
      </Box>
      {docRow ? (
        <Box>
          <Text dimColor>{docRow}</Text>
        </Box>
      ) : null}
    </Box>
  );
}
