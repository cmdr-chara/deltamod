import { createRoot } from "react-dom/client";

import { DeltamodBootScreen } from "../web/components/DeltamodBootScreen";
import "./preview.css";

type ThemeDefinition = {
  id: string;
  color: string;
  soulColor?: string;
  background: string;
  backgroundVideo?: string;
};

const themeFiles = import.meta.glob<ThemeDefinition>(
  "../web/themes/data/*.theme.json",
  { eager: true, import: "default" },
);

const themes = Object.values(themeFiles).reduce<Record<string, ThemeDefinition>>(
  (result, theme) => {
    result[theme.id] = theme;
    return result;
  },
  {},
);

function Preview() {
  const params = new URLSearchParams(window.location.search);
  const requestedTheme = params.get("theme") || "home";
  const autoPlay = params.get("static") !== "1";
  const theme = themes[requestedTheme] || themes.home;

  return (
    <DeltamodBootScreen
      version="2.0.3-BETA.2"
      autoPlay={autoPlay}
      themeColor={theme.color}
      soulColor={theme.soulColor || theme.color}
      backgroundImage={`/web/themes/img/${theme.background}`}
      backgroundVideo={
        theme.backgroundVideo
          ? `/web/themes/video/${theme.backgroundVideo}`
          : undefined
      }
    />
  );
}

createRoot(document.getElementById("root")!).render(<Preview />);
