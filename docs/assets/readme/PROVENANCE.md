# README product GIF provenance

The original feature GIFs were captured on 2026-08-12 from the repository's Electron development build at a 960 × 600 viewport. They show the real Deltamod Community interface and interactions.

`deltamod-workflow-tour.gif` was assembled on 2026-08-20 from six local Playwright smoke-test screenshots captured from the Electron development build at a 1920 × 1080 viewport with the built-in `base` theme: home, mod list, settings, installation manager, catalogue, and credits. It is a compact slideshow of selected screens, not a continuous user recording.

The Mod Shop capture intercepts catalogue requests locally and displays clearly labelled demo entries. It does not depict live catalogue results or claim that the sample mods exist. The temporary capture profile contains an empty DELTARUNE installation fixture and no personal user data.

Frames were captured at 8 FPS and exported with FFmpeg using a deterministic 128-color palette, Bayer dithering, differential rectangles, and infinite looping. The workflow tour sources were scaled to a 960 × 540 output so the original 16:9 screens remain complete without black letterbox borders. Exact dimensions, frame counts, sizes, and SHA-256 hashes are recorded in `manifest.json`.
