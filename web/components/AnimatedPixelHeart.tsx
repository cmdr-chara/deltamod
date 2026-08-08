import {
  type CSSProperties,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
} from "react";

import "./AnimatedPixelHeart.css";

type PixelGroup = "outerHeart" | "eyes" | "nose" | "lowerMarks" | "core";

type SpritePixel = {
  x: number;
  y: number;
  group: PixelGroup;
  revealAt: number;
};

type AnimationMode = "run" | "rewind" | "static";

export type AnimatedPixelHeartProps = {
  size?: number | string;
  className?: string;
  autoPlay?: boolean;
  loop?: boolean;
  interactive?: boolean;
  foregroundColor?: string;
  backgroundColor?: string;
};

// Exact 32x32 binary transcription of the supplied 256x256 sprite.
const SPRITE_ROWS = [
  "................................",
  "................................",
  "......######........######......",
  "....##......##....##......##....",
  "...#..........#..#..........#...",
  "..#............##............#..",
  "..#..........................#..",
  ".#..#......................#..#.",
  ".#...###.................##...#.",
  ".#..######............######..#.",
  ".#.....####...####...####.....#.",
  ".#....######.######.######....#.",
  ".#......####.######.####......#.",
  "..#.......##..####..##.......#..",
  "..#............##............#..",
  "...#........................#...",
  "...#.......#........#.......#...",
  "....#.....###......###.....#....",
  "....#....#####....#####....#....",
  ".....#....................#.....",
  "......#..................#......",
  ".......#.....######.....#.......",
  "........#.....####.....#........",
  ".........#.....##.....#.........",
  "..........#..........#..........",
  "...........#........#...........",
  "............#......#............",
  ".............#....#.............",
  "..............#..#..............",
  "...............##...............",
  "................................",
  "................................",
] as const;

const classifyPixel = (x: number, y: number, row: string): PixelGroup => {
  const isTopOutline = y >= 2 && y <= 5;
  const isSideOutline = y >= 6 && (x === row.indexOf("#") || x === row.lastIndexOf("#"));

  if (isTopOutline || isSideOutline) return "outerHeart";
  if (y === 14) return "core";
  if (y >= 10 && y <= 13 && x >= 13 && x <= 18) return "nose";
  if (y >= 7 && y <= 13) return "eyes";
  return "lowerMarks";
};

const BASE_PIXELS = SPRITE_ROWS.flatMap((row, y) =>
  [...row].flatMap((cell, x) =>
    cell === "#" ? [{ x, y, group: classifyPixel(x, y, row) }] : [],
  ),
);

const pixelKey = (x: number, y: number) => `${x},${y}`;

// Distance along the real connected outline gives the perimeter a mirrored,
// seam-to-tip drawing order without inventing any intermediate geometry.
const outerDistances = (() => {
  const outer = new Set(
    BASE_PIXELS.filter((pixel) => pixel.group === "outerHeart").map((pixel) =>
      pixelKey(pixel.x, pixel.y),
    ),
  );
  const distances = new Map<string, number>();
  const queue: Array<[number, number]> = [
    [15, 5],
    [16, 5],
  ];

  queue.forEach(([x, y]) => distances.set(pixelKey(x, y), 0));
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const [x, y] = queue[cursor];
    const distance = distances.get(pixelKey(x, y)) ?? 0;

    for (let dy = -1; dy <= 1; dy += 1) {
      for (let dx = -1; dx <= 1; dx += 1) {
        if (dx === 0 && dy === 0) continue;
        const nextX = x + dx;
        const nextY = y + dy;
        const nextKey = pixelKey(nextX, nextY);
        if (!outer.has(nextKey) || distances.has(nextKey)) continue;
        distances.set(nextKey, distance + 1);
        queue.push([nextX, nextY]);
      }
    }
  }

  return distances;
})();

const revealTime = (x: number, y: number, group: PixelGroup) => {
  const mirrorDistance = Math.abs(x - 15.5);

  switch (group) {
    case "core":
      return 0.32;
    case "nose":
      return 0.61 + (14 - y) * 0.055 + mirrorDistance * 0.006;
    case "eyes":
      return 0.88 + Math.abs(y - 11) * 0.025 + (mirrorDistance - 4.5) * 0.018;
    case "lowerMarks":
      return y <= 18 ? 1.1 + (y - 16) * 0.045 : 1.24 + (y - 21) * 0.055;
    case "outerHeart":
      return 1.45 + (outerDistances.get(pixelKey(x, y)) ?? 28) * 0.045;
  }
};

const PIXELS: ReadonlyArray<SpritePixel> = BASE_PIXELS.map((pixel) => ({
  ...pixel,
  revealAt: revealTime(pixel.x, pixel.y, pixel.group),
}));

const FIRST_REVEAL = Math.min(...PIXELS.map((pixel) => pixel.revealAt));
const LAST_REVEAL = Math.max(...PIXELS.map((pixel) => pixel.revealAt));
const LOCK_TIME = LAST_REVEAL + 0.2;
const IDLE_TIME = LOCK_TIME + 0.34;
const REWIND_DURATION = 0.86;

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));
const modulo = (value: number, period: number) => ((value % period) + period) % period;

/** Exact sprite geometry driven by a deterministic, replayable master timeline. */
export function AnimatedPixelHeart({
  size = 256,
  className = "",
  autoPlay = true,
  loop = true,
  interactive = true,
  foregroundColor = "var(--theme-soul-color, var(--theme-color, #ffffff))",
  backgroundColor = "transparent",
}: AnimatedPixelHeartProps) {
  const rootRef = useRef<HTMLButtonElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const replayRef = useRef<() => void>(() => undefined);

  const replay = useCallback(() => replayRef.current(), []);

  useEffect(() => {
    const root = rootRef.current;
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d", { alpha: true });
    if (!root || !canvas || !context) return;

    context.imageSmoothingEnabled = false;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

    let mode: AnimationMode = autoPlay ? "run" : "static";
    let startedAt = performance.now();
    let rewindStartedAt = 0;
    let hiddenAt: number | null = null;
    let frameId: number | null = null;

    let resolvedForeground = "rgb(255, 255, 255)";
    let resolvedBackground = "rgba(0, 0, 0, 0)";

    const syncPalette = () => {
      const computed = window.getComputedStyle(root);
      resolvedForeground = computed.color;
      // Read the component-owned token instead of the button's computed
      // background. Deltamod themes style generic buttons with !important,
      // which must never turn the transparent pixel canvas into a solid tile.
      resolvedBackground = computed.getPropertyValue("--aph-background").trim()
        || computed.backgroundColor;
    };

    const backgroundIsTransparent = () =>
      resolvedBackground === "transparent" || /rgba\([^)]*,\s*0(?:\.0+)?\s*\)/.test(resolvedBackground);

    const eraseRect = (x: number, y: number, width: number, height: number) => {
      if (backgroundIsTransparent()) {
        context.clearRect(x, y, width, height);
      } else {
        context.fillStyle = resolvedBackground;
        context.fillRect(x, y, width, height);
        context.fillStyle = resolvedForeground;
      }
    };

    const clearFrame = () => {
      syncPalette();
      context.clearRect(0, 0, 32, 32);
      if (!backgroundIsTransparent()) {
        context.fillStyle = resolvedBackground;
        context.fillRect(0, 0, 32, 32);
      }
      context.fillStyle = resolvedForeground;
    };

    const drawPixels = (
      pixels: ReadonlyArray<SpritePixel>,
      offsetX = 0,
      offsetY = 0,
      bandOnly = false,
    ) => {
      pixels.forEach((pixel) => {
        if (bandOnly && !((pixel.y >= 8 && pixel.y <= 10) || (pixel.y >= 16 && pixel.y <= 18) || (pixel.y >= 23 && pixel.y <= 24))) {
          return;
        }
        context.fillRect(pixel.x + offsetX, pixel.y + offsetY, 1, 1);
      });
    };

    const drawExpandedPixels = (
      pixels: ReadonlyArray<SpritePixel>,
      amount: number,
    ) => {
      pixels.forEach((pixel) => {
        const offsetX = pixel.x < 15.5 ? -amount : amount;
        const offsetY = pixel.y < 15.5 ? -amount : amount;
        context.fillRect(pixel.x + offsetX, pixel.y + offsetY, 1, 1);
      });
    };

    const renderFinal = () => {
      root.dataset.running = "false";
      clearFrame();
      drawPixels(PIXELS);
    };

    const renderRun = (time: number) => {
      root.dataset.running = "true";
      const hover = root.matches(":hover");
      const idle = time - IDLE_TIME;
      const ambient = loop && idle >= 0;
      const eyePeriod = hover ? 4.1 : 6.4;
      const eyePhase = ambient ? modulo(idle, eyePeriod) : 0;
      const blink = ambient && eyePhase > eyePeriod - 0.1 && eyePhase < eyePeriod - 0.045;

      let echoOn = false;
      let glitchOn = false;
      let cutsOn = false;
      let glitchShift = 0;
      let impactEcho = 0;

      // Flash-layer arbitration: lock-in owns the accents, then glitches,
      // then the ambient heartbeat. Only one family runs in any frame.
      const lockPhase = time - LOCK_TIME;
      if (lockPhase >= 0 && lockPhase < 0.19) {
        echoOn = lockPhase < 0.065 || (lockPhase > 0.11 && lockPhase < 0.155);
        impactEcho = lockPhase < 0.035 ? 2 : echoOn ? 1 : 0;
        cutsOn = lockPhase > 0.065 && lockPhase < 0.102;
      } else if (
        (time >= 0.96 && time < 1.04) ||
        (time >= 1.82 && time < 1.9)
      ) {
        glitchOn = true;
        cutsOn = true;
        glitchShift = Math.floor(time * 100) % 2 === 0 ? -1 : 1;
      } else if (ambient) {
        const heartbeatPeriod = hover ? 2.15 : 2.8;
        const heartbeat = modulo(idle, heartbeatPeriod);
        echoOn =
          (heartbeat >= 0.06 && heartbeat < 0.105) ||
          (heartbeat >= 0.2 && heartbeat < 0.235);
        if (echoOn) impactEcho = 1;

        const glitchPeriod = hover ? 4.8 : 7.2;
        const spark = modulo(idle, glitchPeriod);
        if (spark > glitchPeriod - 0.075) {
          echoOn = false;
          glitchOn = true;
          cutsOn = spark > glitchPeriod - 0.045;
          glitchShift = spark > glitchPeriod - 0.035 ? 1 : -1;
        }
      }

      clearFrame();
      if (echoOn) {
        if (impactEcho > 0) {
          drawExpandedPixels(PIXELS, impactEcho);
          if (lockPhase >= 0 && lockPhase < 0.045) {
            drawPixels(PIXELS, -1);
            drawPixels(PIXELS, 1);
          }
        } else {
          drawPixels(PIXELS, -1);
          drawPixels(PIXELS, 1);
        }
      }
      if (glitchOn) {
        const formedPixels = PIXELS.filter((pixel) => time >= pixel.revealAt);
        drawPixels(formedPixels, glitchShift, 0, true);
        drawPixels(formedPixels, -glitchShift, 0, true);
      }

      PIXELS.forEach((pixel) => {
        const progress = (time - pixel.revealAt) / 0.13;
        let visible = progress >= 0 && (progress < 0.22 || progress >= 0.42);
        if (time >= LOCK_TIME) visible = true;
        if (pixel.group === "eyes" && blink) visible = false;
        if (!visible) return;

        let arrivalShiftX = 0;
        let arrivalShiftY = 0;
        if (progress >= 0 && progress < 0.22) {
          const direction = pixel.x < 15.5 ? -1 : 1;
          arrivalShiftX = direction * (progress < 0.075 ? 3 : progress < 0.15 ? 2 : 1);
          if (pixel.group === "core" || pixel.group === "nose") {
            arrivalShiftY = progress < 0.11 ? 2 : 1;
          } else if (pixel.group === "lowerMarks") {
            arrivalShiftY = progress < 0.11 ? 2 : 1;
          }
        }
        context.fillRect(
          pixel.x + arrivalShiftX,
          pixel.y + arrivalShiftY,
          1,
          1,
        );
      });

      if (cutsOn) {
        eraseRect(0, 10, 32, 1);
        eraseRect(0, 18, 32, 1);
        eraseRect(0, 24, 32, 1);
      }
    };

    const renderRewind = (progress: number) => {
      root.dataset.running = "true";
      const revealSpan = Math.max(0.001, LAST_REVEAL - FIRST_REVEAL);

      clearFrame();
      const glitchWindow = progress > 0.3 && progress < 0.38;
      const remainingPixels = PIXELS.filter((pixel) => {
        const revealRank = (pixel.revealAt - FIRST_REVEAL) / revealSpan;
        const hideAt = 0.1 + (1 - revealRank) * 0.72;
        return progress < hideAt;
      });
      if (glitchWindow) {
        drawPixels(remainingPixels, progress < 0.34 ? 1 : -1, 0, true);
        drawPixels(remainingPixels, progress < 0.34 ? -1 : 1, 0, true);
      }
      drawPixels(remainingPixels);

      if (glitchWindow) {
        eraseRect(0, 10, 32, 1);
        eraseRect(0, 18, 32, 1);
      }
    };

    const requestFrame = () => {
      if (frameId === null) frameId = requestAnimationFrame(frame);
    };

    const beginRun = () => {
      mode = "run";
      startedAt = performance.now();
      requestFrame();
    };

    const beginRewind = () => {
      if (reduceMotion.matches || mode === "rewind") return;
      mode = "rewind";
      rewindStartedAt = performance.now();
      requestFrame();
    };

    function frame(now: number) {
      frameId = null;

      if (reduceMotion.matches) {
        mode = "static";
        renderFinal();
        return;
      }

      if (mode === "rewind") {
        const progress = clamp01((now - rewindStartedAt) / (REWIND_DURATION * 1000));
        renderRewind(progress);
        if (progress >= 1) beginRun();
        else requestFrame();
        return;
      }

      if (mode === "static") {
        renderFinal();
        return;
      }

      const time = (now - startedAt) / 1000;
      renderRun(time);
      if (!loop && time > IDLE_TIME + 0.25) {
        renderFinal();
        return;
      }
      requestFrame();
    }

    replayRef.current = () => {
      if (reduceMotion.matches || mode === "rewind") return;
      if (mode === "run" && (performance.now() - startedAt) / 1000 < LOCK_TIME) return;
      beginRewind();
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        hiddenAt = performance.now();
        return;
      }
      if (hiddenAt !== null) {
        const away = performance.now() - hiddenAt;
        startedAt += away;
        rewindStartedAt += away;
        hiddenAt = null;
      }
      requestFrame();
    };

    const onMotionPreferenceChange = () => {
      if (reduceMotion.matches) {
        mode = "static";
        renderFinal();
      } else if (autoPlay) {
        beginRun();
      }
    };

    document.addEventListener("visibilitychange", onVisibilityChange);
    reduceMotion.addEventListener("change", onMotionPreferenceChange);

    if (reduceMotion.matches || !autoPlay) renderFinal();
    else requestFrame();

    return () => {
      replayRef.current = () => undefined;
      if (frameId !== null) cancelAnimationFrame(frameId);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reduceMotion.removeEventListener("change", onMotionPreferenceChange);
    };
  }, [autoPlay, loop]);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!interactive) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      replay();
    }
  };

  const rootStyle = {
    "--aph-size": typeof size === "number" ? `${size}px` : size,
    "--aph-foreground": foregroundColor,
    "--aph-background": backgroundColor,
  } as CSSProperties;

  return (
    <button
      ref={rootRef}
      type="button"
      className={`animated-pixel-heart${className ? ` ${className}` : ""}`}
      style={rootStyle}
      data-running={autoPlay ? "true" : "false"}
      data-interactive={interactive ? "true" : "false"}
      aria-label={interactive ? "Replay animated pixel heart emblem" : "Pixel heart emblem"}
      disabled={!interactive}
      onClick={interactive ? replay : undefined}
      onKeyDown={onKeyDown}
    >
      <canvas
        ref={canvasRef}
        width="32"
        height="32"
        role="img"
        aria-label="Pixel-art heart and skull emblem"
      />
    </button>
  );
}

export default AnimatedPixelHeart;
