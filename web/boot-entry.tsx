import { createRoot, type Root } from "react-dom/client";

import DeltamodBootScreen from "./components/DeltamodBootScreen";

type BootTheme = {
  themeColor?: string;
  soulColor?: string;
  backgroundImage?: string;
  backgroundVideo?: string;
  readyAtVideoTime?: number;
};

type BootApi = {
  setProgress: (progress: number, status?: string) => void;
  setTheme: (theme: BootTheme) => void;
  finish: () => void;
  fail: (message?: string) => void;
};

declare global {
  interface Window {
    DeltamodBoot?: BootApi;
  }
}

const host = document.getElementById("deltamod-boot-root");

if (host) {
  document.body.classList.add("deltamod-boot-active");
  const root: Root = createRoot(host);
  let state = {
    progress: 0,
    status: "Starting local runtime",
    themeReady: false,
    themeColor: "transparent",
    soulColor: "transparent",
    backgroundImage: undefined as string | undefined,
    backgroundVideo: undefined as string | undefined,
    readyAtVideoTime: undefined as number | undefined,
  };
  let finishRequested = false;
  let finishTimer: number | null = null;

  const render = () => {
    if (!state.themeReady) {
      root.render(null);
      return;
    }

    root.render(
      <DeltamodBootScreen
        progress={state.progress}
        status={state.status}
        themeColor={state.themeColor}
        soulColor={state.soulColor}
        backgroundImage={state.backgroundImage}
        backgroundVideo={state.backgroundVideo}
        readyAtVideoTime={state.readyAtVideoTime}
        readyVideoElementId="theme-background-video"
        minimumDuration={5200}
        autoPlay
        onReady={() => {
          if (!finishRequested) return;

          host.dataset.dismissed = "true";
          host.setAttribute("aria-hidden", "true");
          document.body.classList.remove("deltamod-boot-active");
          document.body.classList.add("deltamod-ui-entering");
          if (finishTimer !== null) window.clearTimeout(finishTimer);
          finishTimer = window.setTimeout(() => {
            root.unmount();
            host.replaceChildren();
            host.hidden = true;
            document.body.classList.remove("deltamod-ui-entering");
          }, 1250);
        }}
      />,
    );
  };

  const api: BootApi = {
    setProgress(progress, status) {
      if (finishRequested) return;
      const numericProgress = Number(progress);
      state.progress = Number.isFinite(numericProgress)
        ? Math.max(0, Math.min(1, numericProgress))
        : state.progress;
      if (typeof status === "string" && status.trim()) state.status = status;
      render();
    },

    setTheme(theme) {
      if (finishRequested) return;
      const accentColor = theme.soulColor || theme.themeColor;
      if (accentColor) {
        state.themeColor = accentColor;
        state.soulColor = accentColor;
      }
      state.themeReady = true;
      state.backgroundImage = theme.backgroundImage;
      state.backgroundVideo = theme.backgroundVideo;
      state.readyAtVideoTime = Number.isFinite(theme.readyAtVideoTime)
        ? Math.max(0, Number(theme.readyAtVideoTime))
        : undefined;
      render();
    },

    finish() {
      if (finishRequested) return;
      finishRequested = true;
      state.progress = 1;
      // The component still enforces its cinematic minimum duration. Do not
      // announce Ready while the displayed progress is catching up to 100%.
      state.status = "Opening your session";
      render();
    },

    fail(message = "Ready") {
      if (!state.themeReady) {
        state.themeReady = true;
        state.themeColor = "#ffffff";
        state.soulColor = "#ffffff";
      }
      state.status = message;
      state.progress = 1;
      finishRequested = true;
      render();
    },
  };

  window.DeltamodBoot = api;
  host.hidden = false;
  host.removeAttribute("aria-hidden");
  render();
}

export {};
