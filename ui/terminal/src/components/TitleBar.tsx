import React from "react";
import { Box, Text } from "ink";

interface TitleBarProps {
  mode: string;
  wizardStep?: { current: number; total: number; name: string };
  width: number;
}

export function TitleBar({ mode, wizardStep, width }: TitleBarProps) {
  const left = " rollio";
  const right = ` ${mode} `;
  const center = wizardStep
    ? `Setup: Step ${wizardStep.current}/${wizardStep.total} ${wizardStep.name}`
    : "";

  // Calculate padding
  const usedWidth = left.length + right.length + center.length;
  const remainingWidth = Math.max(0, width - usedWidth);

  return (
    <Box width={width}>
      <Text bold color="cyan">
        {left}
      </Text>
      {center ? (
        <>
          <Text>{" ".repeat(Math.floor(remainingWidth / 2))}</Text>
          <Text color="yellow">{center}</Text>
          <Text>{" ".repeat(Math.ceil(remainingWidth / 2))}</Text>
        </>
      ) : (
        <Text>{" ".repeat(remainingWidth)}</Text>
      )}
      <Text color="green">{right}</Text>
    </Box>
  );
}
