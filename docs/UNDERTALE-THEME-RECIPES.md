# Bundled UNDERTALE theme sources

This inventory defines the closed source set used to generate the eight bundled
UNDERTALE themes. Players receive these themes as built-ins and do not need to run an
extractor, select a game directory, or import the themes themselves.

## Extraction contract

- A maintainer passes the game and cache roots explicitly to
  `scripts/generate-bundled-game-themes.ps1`; no machine-specific source path is
  stored in manifests or provenance.
- Reviewed backgrounds and sprite frames are exported from the maintainer-owned game
  data into the source cache. Clean room maps may also be cached from the documented
  Undertale Wiki source when a complete room render is materially better than a sprite
  collage. The generator accepts only the fixed identifiers below.
- Composition is deterministic. Wide composite scenes use a 320×180 sRGB RGBA canvas,
  integer coordinates and nearest-neighbor sampling followed by a 4× export. Curated
  full-room scenes retain their native canvas and aspect ratio instead of being
  stretched or cropped to 16:9. Music is copied byte-for-byte.
- Publication writes the reviewed manifest, PNG, and Ogg files under `web/themes`.
  Provenance schema v2 records every scene-source identifier and SHA-256 hash plus the
  packaged PNG/Ogg hashes.

## Closed recipe set

| Theme ID | Music | Exact graphic selectors | UI / SOUL color |
| --- | --- | --- | --- |
| `undertale-ruins` | `mus_ruins.ogg` | curated complete Ruins room, 640×480 | `#A13DAD` / `#FF0000` |
| `undertale-snowdin` | `mus_snowy.ogg` | Snowdin Town location render, 640×480 | `#5FCDE4` / `#003CFF` |
| `undertale-waterfall` | `mus_waterfall.ogg` | Echo Flower path location render, 640×480 | `#4568D4` / `#42FCFF` |
| `undertale-void` | `mus_barrier.ogg` | Barrier location render, 640×480 | `#695684` / `#FFFF00` |
| `undertale-hotland` | `mus_anothermedium.ogg` | Hotland CORE-view location render, 640×480 | `#F26A2E` / `#FCA600` |
| `undertale-core` | `mus_core.ogg` | CORE location render, 640×480 | `#405FCA` / `#42FCFF` |
| `undertale-true-lab` | `mus_hereweare.ogg` | DT Extraction Machine location render, 640×480 | `#6E6282` / `#FF0000` |
| `undertale-new-home` | `mus_endarea_parta.ogg` | curated New Home city panorama, 640×480 | `#9B6A1D` / `#FFFF00` |

The stable internal ID `undertale-void` is retained for existing user preferences,
but the visible theme and its assets are now the canonical Barrier location.

The clean room maps are sourced from the Undertale Wiki file pages and curated outside
the repository under `theme-source-cache/selected-scenes`. Provenance records the
exact selected-scene hashes used by the build:

- `https://undertale.wiki/w/File:Ruins_location_entrance.png`
- `https://undertale.wiki/w/File:Snowdin_Town_location.png`
- `https://undertale.wiki/w/File:Waterfall_location_Echo_Flower_path.png`
- `https://undertale.wiki/w/File:Barrier_screenshot.png`
- `https://undertale.wiki/w/File:Hotland_location_Core_View.png`
- `https://undertale.wiki/w/File:CORE_location.png`
- `https://undertale.wiki/w/File:True_Lab_location_DT_Extraction_Machine.png`
- `https://undertale.wiki/w/File:New_Home_location.png`

All eight selected outputs are the corresponding 640×480 clean location renders from
the Undertale Wiki `Location files` catalogue; no character/sprite collage is generated.

New Home uses the complete monochrome city panorama rather than a synthetic hallway
composition. Local room/controller evidence associates `mus_endarea_parta.ogg` with
the castle front and house. `mus_endarea_partb.ogg` begins later in the basement, so
the generator does not concatenate or transcode the two files.

## Deferred locations

Last Corridor is not guessed from filenames alone. It enters the closed set only
after room-resource evidence identifies exact selectors and music identity under the
same deterministic contract.
