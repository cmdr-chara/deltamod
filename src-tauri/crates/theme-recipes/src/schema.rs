use serde::{Deserialize, Serialize};

// Version 1 was pre-release and never persisted or published. Persistent writes have not begun,
// so these closed contracts move together to version 2 without a data migration path.
pub const RECIPE_SCHEMA_VERSION: u32 = 2;
pub const THEME_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const PROVENANCE_SCHEMA_VERSION: u32 = 2;
pub const EXTRACTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RightsAttestation {
    pub user_selected_installation: bool,
    pub authorized_to_extract: bool,
    pub local_only: bool,
}

impl RightsAttestation {
    #[must_use]
    pub const fn accepted() -> Self {
        Self {
            user_selected_installation: true,
            authorized_to_extract: true,
            local_only: true,
        }
    }

    pub(crate) const fn is_accepted(self) -> bool {
        self.user_selected_installation && self.authorized_to_extract && self.local_only
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeId {
    CardCastle,
    Noelle,
    TvWorld,
    TheKnight,
    UndertaleRuins,
    UndertaleSnowdin,
    UndertaleWaterfall,
    UndertaleVoid,
    UndertaleHotland,
    UndertaleCore,
    UndertaleTrueLab,
    UndertaleNewHome,
}

impl RecipeId {
    pub const ALL: [Self; 12] = [
        Self::CardCastle,
        Self::Noelle,
        Self::TvWorld,
        Self::TheKnight,
        Self::UndertaleRuins,
        Self::UndertaleSnowdin,
        Self::UndertaleWaterfall,
        Self::UndertaleVoid,
        Self::UndertaleHotland,
        Self::UndertaleCore,
        Self::UndertaleTrueLab,
        Self::UndertaleNewHome,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CardCastle => "card-castle",
            Self::Noelle => "noelle",
            Self::TvWorld => "tv-world",
            Self::TheKnight => "the-knight",
            Self::UndertaleRuins => "undertale-ruins",
            Self::UndertaleSnowdin => "undertale-snowdin",
            Self::UndertaleWaterfall => "undertale-waterfall",
            Self::UndertaleVoid => "undertale-void",
            Self::UndertaleHotland => "undertale-hotland",
            Self::UndertaleCore => "undertale-core",
            Self::UndertaleTrueLab => "undertale-true-lab",
            Self::UndertaleNewHome => "undertale-new-home",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::CardCastle => "Card Castle",
            Self::Noelle => "Noelle",
            Self::TvWorld => "TV World",
            Self::TheKnight => "The Knight",
            Self::UndertaleRuins => "Ruins",
            Self::UndertaleSnowdin => "Snowdin",
            Self::UndertaleWaterfall => "Waterfall",
            Self::UndertaleVoid => "Void",
            Self::UndertaleHotland => "Hotland",
            Self::UndertaleCore => "CORE",
            Self::UndertaleTrueLab => "True Lab",
            Self::UndertaleNewHome => "New Home",
        }
    }

    #[must_use]
    pub const fn requested_music_name(self) -> &'static str {
        match self {
            Self::CardCastle => "card_castle.ogg",
            Self::Noelle => "noelle.ogg",
            Self::TvWorld => "tv_world.ogg",
            Self::TheKnight => "knight.ogg",
            Self::UndertaleRuins => "mus_ruins.ogg",
            Self::UndertaleSnowdin => "mus_snowy.ogg",
            Self::UndertaleWaterfall => "mus_waterfall.ogg",
            Self::UndertaleVoid => "mus_st_him.ogg",
            Self::UndertaleHotland => "mus_anothermedium.ogg",
            Self::UndertaleCore => "mus_core.ogg",
            Self::UndertaleTrueLab => "mus_hereweare.ogg",
            Self::UndertaleNewHome => "mus_endarea_parta.ogg",
        }
    }

    #[must_use]
    pub(crate) const fn source_work(self) -> &'static str {
        match self {
            Self::CardCastle | Self::Noelle | Self::TvWorld | Self::TheKnight => "DELTARUNE",
            Self::UndertaleRuins
            | Self::UndertaleSnowdin
            | Self::UndertaleWaterfall
            | Self::UndertaleVoid
            | Self::UndertaleHotland
            | Self::UndertaleCore
            | Self::UndertaleTrueLab
            | Self::UndertaleNewHome => "UNDERTALE",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceSlot {
    Background,
    Music,
}

impl SourceSlot {
    pub const ALL: [Self; 2] = [Self::Background, Self::Music];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorStatus {
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorKind {
    BackgroundSprite,
    MusicTrack,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectorPlaceholder {
    pub status: SelectorStatus,
    pub kind: SelectorKind,
    pub requested_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeSelectors {
    pub background: SelectorPlaceholder,
    pub music: SelectorPlaceholder,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformKind {
    CopyVerified,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transform {
    pub kind: TransformKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeTransforms {
    pub background: Vec<Transform>,
    pub music: Vec<Transform>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputScope {
    CustomThemes,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Ogg,
    Wav,
}

impl AudioFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Ogg => "ogg",
            Self::Wav => "wav",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeOutput {
    pub scope: OutputScope,
    pub audio_format: AudioFormat,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attribution {
    pub source_work: String,
    pub notice: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeDefinition {
    pub schema_version: u32,
    pub id: RecipeId,
    pub name: String,
    pub selectors: RecipeSelectors,
    pub transforms: RecipeTransforms,
    pub output: RecipeOutput,
    pub attribution: Attribution,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    Png,
    Ogg,
    Wav,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeManifest {
    pub schema_version: u32,
    pub id: RecipeId,
    pub name: String,
    pub description: String,
    pub built_in: bool,
    pub icon: Option<String>,
    pub music: Option<String>,
    pub local_only: bool,
}
