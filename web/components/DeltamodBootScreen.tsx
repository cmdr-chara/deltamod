import { type CSSProperties, useEffect, useRef } from "react";

import { AnimatedPixelHeart } from "./AnimatedPixelHeart";
import "./DeltamodBootScreen.css";

export type DeltamodBootScreenProps = {
  /** Normalized real loading progress. Omit it to run the built-in demo timeline. */
  progress?: number;
  /** Optional real loader message. */
  status?: string;
  version?: string;
  minimumDuration?: number;
  autoPlay?: boolean;
  className?: string;
  themeColor?: string;
  soulColor?: string;
  backgroundImage?: string;
  backgroundVideo?: string;
  /** Hold the final lock until the active theme video reaches this timestamp. */
  readyAtVideoTime?: number;
  /** Optional host video whose clock should drive readyAtVideoTime. */
  readyVideoElementId?: string;
  onReady?: () => void;
};

const CELL_COUNT = 32;

const STATUS_STEPS = [
  { threshold: 0, text: "Starting local runtime" },
  { threshold: 0.12, text: "Looking for installed games" },
  { threshold: 0.3, text: "Reading mod sources" },
  { threshold: 0.5, text: "Checking patch tools" },
  { threshold: 0.7, text: "Preparing file overlay" },
  { threshold: 0.88, text: "Opening your session" },
  { threshold: 0.999, text: "Ready" },
] as const;

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));
const modulo = (value: number, period: number) => ((value % period) + period) % period;
const deterministicNoise = (value: number) => {
  const result = Math.sin(value * 12.9898) * 43758.5453;
  return result - Math.floor(result);
};
const smooth = (value: number) => {
  const t = clamp01(value);
  return t * t * (3 - 2 * t);
};

const demoProgressAt = (time: number) => {
  const keys = [
    [0, 0],
    [0.55, 0.035],
    [1.2, 0.14],
    [2.05, 0.36],
    [2.85, 0.56],
    [3.45, 0.64],
    [4.15, 0.86],
    [5.25, 1],
  ] as const;

  for (let index = 0; index < keys.length - 1; index += 1) {
    const [t0, p0] = keys[index];
    const [t1, p1] = keys[index + 1];
    if (time <= t1) return p0 + (p1 - p0) * smooth((time - t0) / (t1 - t0));
  }
  return 1;
};

const statusForProgress = (progress: number) => {
  let result: string = STATUS_STEPS[0].text;
  STATUS_STEPS.forEach((step) => {
    if (progress >= step.threshold) result = step.text;
  });
  return result;
};

export function DeltamodBootScreen({
  progress,
  status,
  version = "COMMUNITY BUILD",
  minimumDuration = 5200,
  autoPlay = true,
  className = "",
  themeColor = "var(--theme-color, rgb(205, 68, 81))",
  soulColor = "var(--theme-soul-color, var(--theme-color, #ff0000))",
  backgroundImage,
  backgroundVideo,
  readyAtVideoTime,
  readyVideoElementId,
  onReady,
}: DeltamodBootScreenProps) {
  const rootRef = useRef<HTMLElement>(null);
  const effectsCanvasRef = useRef<HTMLCanvasElement>(null);
  const statusRef = useRef<HTMLSpanElement>(null);
  const percentRef = useRef<HTMLSpanElement>(null);
  const progressCellsRef = useRef<Array<HTMLSpanElement | null>>([]);
  const progressPropRef = useRef(progress);
  const statusPropRef = useRef(status);
  const onReadyRef = useRef(onReady);
  const readyAtVideoTimeRef = useRef(readyAtVideoTime);
  const readyVideoElementIdRef = useRef(readyVideoElementId);

  useEffect(() => {
    progressPropRef.current = progress;
  }, [progress]);

  useEffect(() => {
    statusPropRef.current = status;
  }, [status]);

  useEffect(() => {
    onReadyRef.current = onReady;
  }, [onReady]);

  useEffect(() => {
    readyAtVideoTimeRef.current = readyAtVideoTime;
    readyVideoElementIdRef.current = readyVideoElementId;
  }, [readyAtVideoTime, readyVideoElementId]);

  useEffect(() => {
    const root = rootRef.current;
    const effectsCanvas = effectsCanvasRef.current;
    const effectsContext = effectsCanvas?.getContext("2d", { alpha: true });
    if (!root) return;

    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frameId: number | null = null;
    let startedAt = performance.now();
    let lastFrame = startedAt;
    let hiddenAt: number | null = null;
    let displayedProgress = autoPlay ? 0 : 1;
    let readySent = false;
    let readyAt: number | null = null;
    let previousStatus = "";
    let statusChangedAt = startedAt;
    let statusChangeCount = 0;
    let effectTheme = "rgb(255, 255, 255)";
    let effectSoul = "rgb(255, 255, 255)";
    let paletteSyncedAt = -Infinity;

    const resizeEffects = () => {
      if (!effectsCanvas || !effectsContext) return;
      const bounds = root.getBoundingClientRect();
      const cellSize = Math.max(
        4,
        Math.min(10, Math.round(Math.min(bounds.width / 150, bounds.height / 90))),
      );
      const width = Math.max(1, Math.ceil(bounds.width / cellSize));
      const height = Math.max(1, Math.ceil(bounds.height / cellSize));

      if (effectsCanvas.width !== width || effectsCanvas.height !== height) {
        effectsCanvas.width = width;
        effectsCanvas.height = height;
        effectsContext.imageSmoothingEnabled = false;
      }
    };

    const syncEffectPalette = (now: number) => {
      if (!effectsCanvas || now - paletteSyncedAt < 240) return;
      const style = window.getComputedStyle(effectsCanvas);
      effectTheme = style.color;
      effectSoul = style.borderTopColor;
      paletteSyncedAt = now;
    };

    const drawEffectFrame = (
      now: number,
      elapsed: number,
      glitchMode: string,
      readyAge: number,
    ) => {
      if (!effectsCanvas || !effectsContext || reduceMotion.matches || !autoPlay) return;

      resizeEffects();
      syncEffectPalette(now);
      const width = effectsCanvas.width;
      const height = effectsCanvas.height;
      const centerX = Math.floor(width / 2);
      const centerY = Math.floor(height * 0.5);
      const extent = Math.min(width, height);
      effectsContext.clearRect(0, 0, width, height);

      // Video themes must glitch their live frame. Reusing the poster-backed
      // CSS slices would flash the thumbnail whenever a band tears.
      const sceneVideo = root.querySelector<HTMLVideoElement>(".db-scene-video");
      if (
        glitchMode !== "off"
        && sceneVideo
        && sceneVideo.readyState >= 2
        && sceneVideo.videoWidth > 0
        && sceneVideo.videoHeight > 0
      ) {
        const bands = {
          a: { top: 0.19, height: 0.11, offset: 2 },
          b: { top: 0.47, height: 0.1, offset: -3 },
          c: { top: 0.72, height: 0.08, offset: 2 },
        } as const;
        const band = bands[glitchMode as keyof typeof bands];
        if (band) {
          const videoAspect = sceneVideo.videoWidth / sceneVideo.videoHeight;
          const canvasAspect = width / height;
          let sourceX = 0;
          let sourceY = 0;
          let sourceWidth = sceneVideo.videoWidth;
          let sourceHeight = sceneVideo.videoHeight;
          if (videoAspect > canvasAspect) {
            sourceWidth = sceneVideo.videoHeight * canvasAspect;
            sourceX = (sceneVideo.videoWidth - sourceWidth) / 2;
          } else {
            sourceHeight = sceneVideo.videoWidth / canvasAspect;
            sourceY = (sceneVideo.videoHeight - sourceHeight) / 2;
          }

          effectsContext.save();
          effectsContext.globalAlpha = 0.74;
          effectsContext.imageSmoothingEnabled = false;
          effectsContext.drawImage(
            sceneVideo,
            sourceX,
            sourceY + sourceHeight * band.top,
            sourceWidth,
            sourceHeight * band.height,
            band.offset,
            Math.round(height * band.top),
            width,
            Math.max(1, Math.round(height * band.height)),
          );
          effectsContext.restore();
        }
      }

      const pixel = (
        x: number,
        y: number,
        color: string,
        alpha = 1,
        pixelWidth = 1,
        pixelHeight = 1,
      ) => {
        effectsContext.globalAlpha = clamp01(alpha);
        effectsContext.fillStyle = color;
        effectsContext.fillRect(
          Math.round(x),
          Math.round(y),
          Math.max(1, Math.round(pixelWidth)),
          Math.max(1, Math.round(pixelHeight)),
        );
      };

      const diamond = (radius: number, color: string, alpha: number, spacing = 2) => {
        for (let x = -radius; x <= radius; x += spacing) {
          const y = radius - Math.abs(x);
          pixel(centerX + x, centerY + y, color, alpha);
          if (y !== 0) pixel(centerX + x, centerY - y, color, alpha);
        }
      };

      // The scene decompresses in coarse blocks from the emblem outwards.
      // This is drawn before all light effects so the signal stays visible.
      if (elapsed < 1.72) {
        const reveal = smooth(clamp01((elapsed - 0.22) / 1.42));
        for (let y = 0; y < height; y += 4) {
          for (let x = 0; x < width; x += 6) {
            const distanceFromCore = Math.abs(y - centerY) / Math.max(1, height * 0.5);
            const localReveal = clamp01(reveal * 1.2 - distanceFromCore * 0.24);
            const noise = deterministicNoise(x * 37 + y * 71);
            if (noise > localReveal) pixel(x, y, "rgb(8, 8, 8)", 1, 6, 4);
          }
        }
      }

      // The first core pixel sends a symmetrical, low-resolution signal burst
      // through the otherwise black scene.
      if (elapsed >= 0.18 && elapsed < 1.16) {
        for (let ray = 0; ray < 24; ray += 1) {
          const progress = clamp01((elapsed - 0.18 - (ray % 4) * 0.03) / 0.78);
          if (progress <= 0 || progress >= 1) continue;
          const angle = (ray / 24) * Math.PI * 2;
          const distance = progress * extent * 0.36;
          const color = ray % 4 === 0 ? effectTheme : effectSoul;
          const alpha = Math.sin(progress * Math.PI);
          for (let trail = 0; trail < 3; trail += 1) {
            const trailDistance = distance - trail * 2.4;
            pixel(
              centerX + Math.cos(angle) * trailDistance,
              centerY + Math.sin(angle) * trailDistance * 0.62,
              color,
              alpha * (1 - trail * 0.28),
              trail === 0 && ray % 6 === 0 ? 2 : 1,
            );
          }
        }
      }

      // Pixel fragments converge on the real sprite while its outline is drawn.
      if (elapsed >= 0.92 && elapsed < 2.82) {
        for (let fragment = 0; fragment < 32; fragment += 1) {
          const progress = clamp01(
            (elapsed - 0.92 - (fragment % 8) * 0.065) / 1.1,
          );
          if (progress <= 0 || progress >= 1) continue;
          const angle = (fragment / 32) * Math.PI * 2;
          const start = extent * (0.46 + (fragment % 3) * 0.025);
          const end = extent * 0.235;
          const distance = start + (end - start) * smooth(progress);
          const color = fragment % 5 === 0 ? effectTheme : effectSoul;
          const alpha = Math.sin(progress * Math.PI) * 0.86;
          for (let trail = 0; trail < 3; trail += 1) {
            const trailDistance = distance + trail * 2;
            pixel(
              centerX + Math.cos(angle) * trailDistance,
              centerY + Math.sin(angle) * trailDistance * 0.7,
              color,
              alpha * (1 - trail * 0.3),
            );
          }
        }
      }

      // Three staggered diamond waves make the emblem lock feel mechanical.
      for (let wave = 0; wave < 3; wave += 1) {
        const lockProgress = (elapsed - (2.62 + wave * 0.085)) / 0.64;
        if (lockProgress < 0 || lockProgress >= 1) continue;
        const radius = Math.max(
          4,
          Math.round(extent * (0.22 + lockProgress * (0.18 + wave * 0.025))),
        );
        diamond(
          radius,
          wave === 1 ? effectTheme : effectSoul,
          (1 - lockProgress) * (0.92 - wave * 0.14),
          wave === 2 ? 3 : 2,
        );
      }

      if (glitchMode !== "off") {
        const modeOffset = glitchMode === "a" ? 17 : glitchMode === "b" ? 41 : 73;
        const frame = Math.floor(now / 16);
        for (let band = 0; band < 5; band += 1) {
          const seed = frame * 19 + band * 31 + modeOffset;
          const y = modulo(seed * 13, height);
          const segmentWidth = 4 + modulo(seed * 7, Math.max(5, Math.floor(width * 0.18)));
          const fromRight = band % 2 === 1;
          const x = fromRight ? width - segmentWidth - modulo(seed, 13) : modulo(seed, 13);
          if (band < 2) pixel(0, y, "rgb(8, 8, 8)", 0.96, width);
          pixel(x, y, band % 3 === 0 ? effectSoul : effectTheme, 0.88, segmentWidth);
        }
      }

      // Completion is a larger but very short burst, then the screen goes quiet.
      if (readyAge >= 0 && readyAge < 0.82) {
        const progress = clamp01(readyAge / 0.82);
        if (readyAge < 0.055) {
          pixel(0, 0, effectSoul, 0.82, width, height);
        } else if (readyAge < 0.14) {
          for (let y = 0; y < height; y += 2) {
            pixel(0, y, y % 4 === 0 ? effectSoul : effectTheme, 0.42, width);
          }
        }

        for (let wave = 0; wave < 3; wave += 1) {
          const waveProgress = clamp01((readyAge - wave * 0.055) / 0.7);
          if (readyAge < wave * 0.055) continue;
          diamond(
            Math.round(extent * (0.2 + smooth(waveProgress) * 0.5)),
            wave === 1 ? effectTheme : effectSoul,
            (1 - waveProgress) * 0.84,
            2 + wave,
          );
        }

        for (let ray = 0; ray < 44; ray += 1) {
          const angle = (ray / 44) * Math.PI * 2;
          const distance = extent * (0.22 + smooth(progress) * 0.42);
          const color = ray % 3 === 0 ? effectTheme : effectSoul;
          for (let trail = 0; trail < 3; trail += 1) {
            const trailDistance = distance - trail * 3;
            pixel(
              centerX + Math.cos(angle) * trailDistance,
              centerY + Math.sin(angle) * trailDistance * 0.66,
              color,
              (1 - progress) * (1 - trail * 0.27),
              trail === 0 && ray % 4 === 0 ? 2 : 1,
            );
          }
        }
      } else if (elapsed > 3.35) {
        const idlePhase = modulo(elapsed - 3.35, 3.1);
        if (idlePhase < 0.16) {
          for (let spark = 0; spark < 10; spark += 1) {
            const side = spark % 2 === 0 ? -1 : 1;
            pixel(
              centerX + side * (extent * 0.25 + spark * 2),
              centerY - 13 + spark * 3,
              spark % 3 === 0 ? effectTheme : effectSoul,
              1 - idlePhase / 0.16,
            );
          }
        }
      }

      effectsContext.globalAlpha = 1;
    };

    const resizeObserver = new ResizeObserver(resizeEffects);
    resizeObserver.observe(root);
    resizeEffects();

    const render = (now: number) => {
      frameId = null;
      const elapsed = (now - startedAt) / 1000;
      const delta = Math.min(0.05, (now - lastFrame) / 1000);
      lastFrame = now;
      const controlled = progressPropRef.current;
      const rawTarget = controlled === undefined
        ? autoPlay ? demoProgressAt(elapsed) : 1
        : clamp01(controlled);
      const timeGate = autoPlay ? clamp01((now - startedAt) / Math.max(1, minimumDuration)) : 1;
      const target = Math.min(rawTarget, timeGate);

      if (reduceMotion.matches || !autoPlay) {
        displayedProgress = target;
      } else {
        const next = displayedProgress + (target - displayedProgress) * Math.min(1, delta * 5.5);
        displayedProgress = Math.max(displayedProgress, next);
        if (target - displayedProgress < 0.0005) displayedProgress = target;
      }

      const cueTime = readyAtVideoTimeRef.current;
      const hostVideoId = readyVideoElementIdRef.current;
      const hostVideo = hostVideoId
        ? document.getElementById(hostVideoId) as HTMLVideoElement | null
        : null;
      const syncVideo = hostVideo || root.querySelector<HTMLVideoElement>(".db-scene-video");
      const videoCueReached = cueTime === undefined
        || (syncVideo !== null && syncVideo.currentTime >= cueTime)
        // Never strand startup if media decoding/autoplay fails.
        || elapsed >= cueTime + 1.5;
      // A themed visual cue is a hard edit point. Once the real loader and the
      // minimum sequence are complete, snap the remaining fractional progress
      // so easing cannot make the lock miss the intended video frame.
      if (
        cueTime !== undefined
        && videoCueReached
        && rawTarget >= 0.999
        && timeGate >= 0.999
      ) {
        displayedProgress = 1;
      }
      const ready = displayedProgress >= 0.999 && timeGate >= 0.999 && videoCueReached;
      if (ready && readyAt === null) readyAt = now;

      let phase = "load";
      if (elapsed < 0.28) phase = "cold";
      else if (elapsed < 0.85) phase = "wake";
      else if (elapsed < 1.5) phase = "probe";
      if (ready) phase = "ready";
      root.dataset.phase = reduceMotion.matches ? ready ? "ready" : "load" : phase;

      const litCells = Math.round(displayedProgress * CELL_COUNT);
      progressCellsRef.current.forEach((cell, index) => {
        if (!cell) return;
        if (ready) cell.dataset.state = "on";
        else if (litCells > 0 && index === litCells - 1) cell.dataset.state = "head";
        else cell.dataset.state = index < litCells ? "on" : "off";
      });
      root.dataset.tick = Math.floor(now / 68) % 2 === 0 ? "on" : "off";

      const desiredStatus = ready
        ? "Ready"
        : statusPropRef.current || statusForProgress(displayedProgress);
      if (desiredStatus !== previousStatus) {
        previousStatus = desiredStatus;
        statusChangedAt = now;
        statusChangeCount += 1;
      }
      if (statusRef.current) {
        const characterCount = reduceMotion.matches || !autoPlay
          ? desiredStatus.length
          : Math.floor((now - statusChangedAt) / 24);
        const typed = desiredStatus.slice(0, characterCount);
        const cursor = characterCount < desiredStatus.length ? "_" : "";
        statusRef.current.textContent = `${typed}${cursor}`;
      }

      if (percentRef.current) {
        percentRef.current.textContent = `${Math.round(displayedProgress * 100)
          .toString()
          .padStart(3, "0")}%`;
      }

      root.dataset.lock =
        readyAt !== null && now - readyAt < 180 ? "flash" : ready ? "settled" : "off";

      let glitchMode = "off";
      const scriptedGlitches = [
        [0.7, 0.79, "a"],
        [1.31, 1.39, "b"],
        [1.86, 1.95, "c"],
        [2.66, 2.79, "a"],
      ] as const;
      for (const [from, to, mode] of scriptedGlitches) {
        if (elapsed >= from && elapsed < to) glitchMode = mode;
      }
      if (statusChangeCount > 1 && now - statusChangedAt < 72) {
        glitchMode = statusChangeCount % 2 === 0 ? "b" : "c";
      }
      if (readyAt !== null && now - readyAt < 130) glitchMode = "a";
      root.dataset.glitch = glitchMode;

      let uiX = 0;
      let uiY = 0;
      if (glitchMode === "a") uiX = -4;
      else if (glitchMode === "b") uiX = 5;
      else if (glitchMode === "c") uiY = -3;

      const lockAge = elapsed - 2.62;
      if (lockAge >= 0 && lockAge < 0.24) {
        const lockKicks = [
          [-6, 1],
          [5, -2],
          [-4, 1],
          [3, 0],
          [-2, 1],
          [0, 0],
        ] as const;
        const kick = lockKicks[Math.min(lockKicks.length - 1, Math.floor(lockAge / 0.04))];
        [uiX, uiY] = kick;
      }

      const readyAge = readyAt === null ? -1 : (now - readyAt) / 1000;
      if (readyAge >= 0 && readyAge < 0.22) {
        const readyKicks = [
          [11, -2],
          [-9, 2],
          [7, -1],
          [-5, 1],
          [3, 0],
          [0, 0],
        ] as const;
        const kick = readyKicks[Math.min(readyKicks.length - 1, Math.floor(readyAge / 0.036))];
        [uiX, uiY] = kick;
      }

      if (reduceMotion.matches || !autoPlay) [uiX, uiY] = [0, 0];
      root.style.setProperty("--db-fx-x", `${uiX}px`);
      root.style.setProperty("--db-fx-y", `${uiY}px`);
      root.style.setProperty("--db-scene-x", `${-uiX * 2}px`);
      root.style.setProperty("--db-scene-y", `${-uiY * 2}px`);
      drawEffectFrame(
        now,
        elapsed,
        glitchMode,
        readyAge,
      );

      if (ready && !readySent) {
        readySent = true;
        onReadyRef.current?.();
      }

      frameId = requestAnimationFrame(render);
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        hiddenAt = performance.now();
      } else if (hiddenAt !== null) {
        const away = performance.now() - hiddenAt;
        startedAt += away;
        statusChangedAt += away;
        if (readyAt !== null) readyAt += away;
        lastFrame = performance.now();
        hiddenAt = null;
      }
    };

    document.addEventListener("visibilitychange", onVisibilityChange);
    frameId = requestAnimationFrame(render);

    return () => {
      if (frameId !== null) cancelAnimationFrame(frameId);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      resizeObserver.disconnect();
    };
  }, [autoPlay, minimumDuration]);

  const encodedBackground = backgroundImage
    ? `url(${JSON.stringify(backgroundImage)})`
    : "none";
  const rootStyle = {
    "--db-theme-color": themeColor,
    "--db-soul-color": soulColor,
    "--db-background-image": encodedBackground,
  } as CSSProperties;

  return (
    <section
      ref={rootRef}
      className={`deltamod-boot${className ? ` ${className}` : ""}`}
      style={rootStyle}
      data-phase={autoPlay ? "cold" : "ready"}
      data-autoplay={autoPlay ? "on" : "off"}
      data-lock="off"
      data-glitch="off"
      data-tick="off"
      data-video={backgroundVideo ? "on" : "off"}
      data-version={version}
      aria-label="Deltamod loading screen"
    >
      <div className="db-scene-base" aria-hidden="true">
        {backgroundVideo && autoPlay ? (
          <video
            className="db-scene-video"
            src={backgroundVideo}
            poster={backgroundImage}
            autoPlay
            muted
            loop
            playsInline
          />
        ) : null}
      </div>
      <div className="db-backdrop" aria-hidden="true" />
      <div className="db-scene-echo" aria-hidden="true">
        <span className="db-scene-slice db-scene-slice-a" />
        <span className="db-scene-slice db-scene-slice-b" />
        <span className="db-scene-slice db-scene-slice-c" />
      </div>
      <div className="db-curtain db-curtain-top" aria-hidden="true" />
      <div className="db-curtain db-curtain-bottom" aria-hidden="true" />
      <canvas
        ref={effectsCanvasRef}
        className="db-effects"
        width="160"
        height="90"
        aria-hidden="true"
      />

      <main className="db-main">
        <h1 className="db-visually-hidden">Starting Deltamod</h1>

        <div className="db-sequence">
          <div className="db-emblem-stage">
            <AnimatedPixelHeart
              size="clamp(220px, 39vmin, 370px)"
              autoPlay={autoPlay}
              loop
              interactive={false}
              foregroundColor="var(--db-soul-color)"
              backgroundColor="transparent"
            />
          </div>

          <div className="db-loading">
            <div className="db-status-line">
              <span ref={statusRef} className="db-status" aria-live="polite" />
              <span ref={percentRef} className="db-percent">000%</span>
            </div>
            <div className="db-progress" aria-hidden="true">
              {Array.from({ length: CELL_COUNT }, (_, index) => (
                <span
                  key={index}
                  ref={(element) => {
                    progressCellsRef.current[index] = element;
                  }}
                  data-state="off"
                />
              ))}
            </div>
          </div>
        </div>
      </main>
    </section>
  );
}

export default DeltamodBootScreen;
