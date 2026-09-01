use deltamod_theme_recipes::RecipeId;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AvailabilityState {
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectorResolution {
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityMetadata {
    pub local_only: bool,
    pub availability: AvailabilityState,
    pub selectors: SelectorResolution,
    pub extraction_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaCard {
    pub recipe_id: RecipeId,
    pub display_name: &'static str,
    pub local_only: bool,
    pub availability: AvailabilityState,
    pub selectors: SelectorResolution,
    pub extraction_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingIntegration {
    pub executor_feature_gated: bool,
    pub create_only_per_operation_staging: bool,
    pub process_shared_operation_lock: bool,
    pub persistent_idempotency_intent: bool,
    pub cancellation_checkpoints: bool,
    pub publication_wired: bool,
    pub lifecycle_publication_required: bool,
}

#[must_use]
pub const fn availability() -> AvailabilityMetadata {
    AvailabilityMetadata {
        local_only: true,
        availability: AvailabilityState::Disabled,
        selectors: SelectorResolution::Unresolved,
        extraction_ready: false,
    }
}

#[must_use]
pub const fn qa_summary() -> [QaCard; RecipeId::ALL.len()] {
    let mut cards = [qa_card(RecipeId::ALL[0]); RecipeId::ALL.len()];
    let mut index = 0;
    while index < RecipeId::ALL.len() {
        cards[index] = qa_card(RecipeId::ALL[index]);
        index += 1;
    }
    cards
}

const fn qa_card(recipe_id: RecipeId) -> QaCard {
    QaCard {
        recipe_id,
        display_name: recipe_id.display_name(),
        local_only: true,
        availability: AvailabilityState::Disabled,
        selectors: SelectorResolution::Unresolved,
        extraction_ready: false,
    }
}

/// Integration contract for the future host adapter.
///
/// `publication_wired` deliberately remains false. Staging success is not publication success;
/// lifecycle-runtime must bind the returned handoff to its operation registry, lease, journal,
/// filesystem boundary, and publication transaction before this feature can be exposed.
#[must_use]
pub const fn staging_integration() -> StagingIntegration {
    StagingIntegration {
        executor_feature_gated: true,
        create_only_per_operation_staging: true,
        process_shared_operation_lock: true,
        persistent_idempotency_intent: true,
        cancellation_checkpoints: true,
        publication_wired: false,
        lifecycle_publication_required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_availability_is_local_disabled_and_unresolved() {
        assert_eq!(
            availability(),
            AvailabilityMetadata {
                local_only: true,
                availability: AvailabilityState::Disabled,
                selectors: SelectorResolution::Unresolved,
                extraction_ready: false,
            }
        );
    }

    #[test]
    fn qa_summary_enumerates_all_metadata_only_cards() {
        let cards = qa_summary();
        assert_eq!(cards.len(), RecipeId::ALL.len());
        assert_eq!(
            cards.map(|card| card.recipe_id),
            RecipeId::ALL,
            "the QA cards must follow the closed recipe set"
        );
        assert_eq!(
            cards.map(|card| card.display_name),
            [
                "Card Castle",
                "Noelle",
                "TV World",
                "The Knight",
                "Ruins",
                "Snowdin",
                "Waterfall",
                "Void",
                "Hotland",
                "CORE",
                "True Lab",
                "New Home",
            ]
        );
        assert!(cards.iter().all(|card| {
            card.local_only
                && card.availability == AvailabilityState::Disabled
                && card.selectors == SelectorResolution::Unresolved
                && !card.extraction_ready
        }));

        let serialized = serde_json::to_value(cards).expect("serialize metadata-only QA cards");
        let serialized_ids = serialized
            .as_array()
            .expect("QA card array")
            .iter()
            .map(|card| card["recipeId"].as_str().expect("serialized recipe ID"))
            .collect::<Vec<_>>();
        assert_eq!(
            serialized_ids,
            [
                "card-castle",
                "noelle",
                "tv-world",
                "the-knight",
                "undertale-ruins",
                "undertale-snowdin",
                "undertale-waterfall",
                "undertale-void",
                "undertale-hotland",
                "undertale-core",
                "undertale-true-lab",
                "undertale-new-home",
            ]
        );

        let json = serde_json::to_string(&serialized).expect("serialize QA card values");
        for forbidden in ["sourcePath", "relativePath", "gameRoot", "stagingRoot"] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn staging_is_create_only_and_publication_is_explicitly_unwired() {
        let integration = staging_integration();
        assert!(integration.executor_feature_gated);
        assert!(integration.create_only_per_operation_staging);
        assert!(integration.process_shared_operation_lock);
        assert!(integration.persistent_idempotency_intent);
        assert!(integration.cancellation_checkpoints);
        assert!(!integration.publication_wired);
        assert!(integration.lifecycle_publication_required);
    }
}
