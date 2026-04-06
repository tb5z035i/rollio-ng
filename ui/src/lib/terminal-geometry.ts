import { spawnSync } from "node:child_process";
import type { WriteStream } from "node:tty";
import {
  DEFAULT_ASCII_CELL_GEOMETRY,
  type AsciiCellGeometry,
} from "./renderers/types.js";

export interface TerminalMetrics {
  columns: number;
  rows: number;
  cellGeometry: AsciiCellGeometry;
}

interface TtyWinsize {
  rows: number;
  cols: number;
  xpixel: number;
  ypixel: number;
}

const TIOCGWINSZ_PYTHON = [
  "import fcntl, json, struct, termios",
  "with open('/dev/tty', 'rb', buffering=0) as tty:",
  "    winsize = fcntl.ioctl(tty.fileno(), termios.TIOCGWINSZ, struct.pack('HHHH', 0, 0, 0, 0))",
  "rows, cols, xpixel, ypixel = struct.unpack('HHHH', winsize)",
  "print(json.dumps({'rows': rows, 'cols': cols, 'xpixel': xpixel, 'ypixel': ypixel}))",
].join("\n");

export function getTerminalMetrics(stdout: WriteStream): TerminalMetrics {
  const fallback = {
    columns: stdout.columns || 80,
    rows: stdout.rows || 24,
  };
  const winsize = readTtyWinsize(stdout);
  return metricsFromWinsize(fallback.columns, fallback.rows, winsize);
}

export function metricsFromWinsize(
  fallbackColumns: number,
  fallbackRows: number,
  winsize: TtyWinsize | null,
): TerminalMetrics {
  if (
    winsize &&
    winsize.cols > 0 &&
    winsize.rows > 0 &&
    winsize.xpixel > 0 &&
    winsize.ypixel > 0
  ) {
    return {
      columns: winsize.cols,
      rows: winsize.rows,
      cellGeometry: {
        pixelWidth: winsize.xpixel / winsize.cols,
        pixelHeight: winsize.ypixel / winsize.rows,
      },
    };
  }

  return {
    columns: fallbackColumns,
    rows: fallbackRows,
    cellGeometry: DEFAULT_ASCII_CELL_GEOMETRY,
  };
}

function readTtyWinsize(stdout: WriteStream): TtyWinsize | null {
  if (!stdout.isTTY || process.platform === "win32") {
    return null;
  }

  const result = spawnSync("python3", ["-c", TIOCGWINSZ_PYTHON], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
    timeout: 200,
  });
  if (result.status !== 0 || !result.stdout) {
    return null;
  }

  try {
    const parsed = JSON.parse(result.stdout) as Partial<TtyWinsize>;
    if (
      typeof parsed.rows === "number" &&
      typeof parsed.cols === "number" &&
      typeof parsed.xpixel === "number" &&
      typeof parsed.ypixel === "number"
    ) {
      return {
        rows: parsed.rows,
        cols: parsed.cols,
        xpixel: parsed.xpixel,
        ypixel: parsed.ypixel,
      };
    }
  } catch {
    return null;
  }

  return null;
}
