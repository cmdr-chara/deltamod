use crate::{
    Attribution, AudioFormat, OutputScope, RecipeDefinition, RecipeError, RecipeId, RecipeOutput,
    RecipeSelectors, RecipeTransforms, Result, SelectorKind, SelectorPlaceholder, SelectorStatus,
    Transform, TransformKind, RECIPE_SCHEMA_VERSION,
};

const ATTRIBUTION_NOTICE: &str =
    "Source rights remain with their respective owners; generated assets are local-only.";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRecipe(RecipeDefinition);

impl ValidatedRecipe {
    #[must_use]
    pub const fn definition(&self) -> &RecipeDefinition {
        &self.0
    }

    #[must_use]
    pub const fn id(&self) -> RecipeId {
        self.0.id
    }
}

#[must_use]
pub fn definition(id: RecipeId) -> RecipeDefinition {
    RecipeDefinition {
        schema_version: RECIPE_SCHEMA_VERSION,
        id,
        name: id.display_name().to_owned(),
        selectors: RecipeSelectors {
            background: SelectorPlaceholder {
                status: SelectorStatus::Unresolved,
                kind: SelectorKind::BackgroundSprite,
                requested_name: None,
            },
            music: SelectorPlaceholder {
                status: SelectorStatus::Unresolved,
                kind: SelectorKind::MusicTrack,
                requested_name: Some(id.requested_music_name().to_owned()),
            },
        },
        transforms: RecipeTransforms {
            background: vec![Transform {
                kind: TransformKind::CopyVerified,
            }],
            music: vec![Transform {
                kind: TransformKind::CopyVerified,
            }],
        },
        output: RecipeOutput {
            scope: OutputScope::CustomThemes,
            audio_format: AudioFormat::Ogg,
        },
        attribution: Attribution {
            source_work: id.source_work().to_owned(),
            notice: ATTRIBUTION_NOTICE.to_owned(),
        },
    }
}

#[must_use]
pub fn definitions() -> [RecipeDefinition; RecipeId::ALL.len()] {
    RecipeId::ALL.map(definition)
}

pub fn validate_recipe(candidate: RecipeDefinition) -> Result<ValidatedRecipe> {
    if candidate.schema_version != RECIPE_SCHEMA_VERSION {
        return Err(RecipeError::UnsupportedSchema(candidate.schema_version));
    }
    if candidate != definition(candidate.id) {
        return Err(RecipeError::NonCanonicalDefinition);
    }
    Ok(ValidatedRecipe(candidate))
}

pub fn parse_and_validate_recipe(json: &[u8]) -> Result<ValidatedRecipe> {
    let candidate = serde_json::from_slice(json).map_err(RecipeError::InvalidRecipeJson)?;
    validate_recipe(candidate)
}
