use crate::{
    plan::pretty_json, AudioFormat, ExecutionPlan, MediaType, OutputArtifact, RecipeId, Result,
    Sha256Digest, SourceIdentifier, SourceSlot, Transform, EXTRACTOR_VERSION,
    PROVENANCE_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RightsMarker {
    UserAuthorizedInstallationLocalOnly,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceSource {
    pub slot: SourceSlot,
    pub identifier: SourceIdentifier,
    pub sha256: Sha256Digest,
    pub media_type: MediaType,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceTransforms {
    pub slot: SourceSlot,
    pub steps: Vec<Transform>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputProvenance {
    pub file: OutputArtifact,
    pub sha256: Sha256Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceDocument {
    pub schema_version: u32,
    pub recipe_id: RecipeId,
    pub extractor_version: String,
    pub rights: RightsMarker,
    pub attribution: crate::Attribution,
    pub sources: Vec<ProvenanceSource>,
    pub transforms: Vec<ProvenanceTransforms>,
    pub outputs: Vec<OutputProvenance>,
}

pub fn build_provenance(plan: &ExecutionPlan) -> Result<ProvenanceDocument> {
    let background = plan.selectors().background();
    let music = plan.selectors().music();
    let theme_hash = Sha256Digest::from_bytes(&plan.theme_json()?);
    let music_artifact = match plan.recipe().definition().output.audio_format {
        AudioFormat::Ogg => OutputArtifact::MusicOgg,
        AudioFormat::Wav => OutputArtifact::MusicWav,
    };

    Ok(ProvenanceDocument {
        schema_version: PROVENANCE_SCHEMA_VERSION,
        recipe_id: plan.recipe().id(),
        extractor_version: EXTRACTOR_VERSION.to_owned(),
        rights: RightsMarker::UserAuthorizedInstallationLocalOnly,
        attribution: plan.recipe().definition().attribution.clone(),
        sources: vec![
            ProvenanceSource {
                slot: SourceSlot::Background,
                identifier: background.identifier().clone(),
                sha256: background.sha256().clone(),
                media_type: background.media_type(),
            },
            ProvenanceSource {
                slot: SourceSlot::Music,
                identifier: music.identifier().clone(),
                sha256: music.sha256().clone(),
                media_type: music.media_type(),
            },
        ],
        transforms: vec![
            ProvenanceTransforms {
                slot: SourceSlot::Background,
                steps: plan.transforms(SourceSlot::Background).to_vec(),
            },
            ProvenanceTransforms {
                slot: SourceSlot::Music,
                steps: plan.transforms(SourceSlot::Music).to_vec(),
            },
        ],
        outputs: vec![
            OutputProvenance {
                file: OutputArtifact::ThemeJson,
                sha256: theme_hash,
            },
            OutputProvenance {
                file: OutputArtifact::BackgroundPng,
                sha256: background.sha256().clone(),
            },
            OutputProvenance {
                file: music_artifact,
                sha256: music.sha256().clone(),
            },
        ],
    })
}

pub fn provenance_json(document: &ProvenanceDocument) -> Result<Vec<u8>> {
    pretty_json(document)
}

pub fn provenance_sha256(document: &ProvenanceDocument) -> Result<Sha256Digest> {
    Ok(Sha256Digest::from_bytes(&provenance_json(document)?))
}
