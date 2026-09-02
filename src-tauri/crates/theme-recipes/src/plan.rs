use crate::{
    path_policy::validate_opened_media, AudioFormat, MediaType, RecipeError, Result,
    RightsAttestation, SelectedGameRoot, Sha256Digest, SourceIdentifier, SourceSlot, ThemeManifest,
    Transform, ValidatedRecipe, ValidatedSelectors, THEME_MANIFEST_SCHEMA_VERSION,
};
use deltamod_product_contracts::ValidatedRelativePath;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io, io::Read, path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadPolicy {
    NoFollow,
}

/// A path-bearing read capability that intentionally has no serde representation.
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<deltamod_theme_recipes::PlannedRead>();
/// ```
#[derive(Clone)]
pub struct PlannedRead {
    pub slot: SourceSlot,
    pub identifier: SourceIdentifier,
    pub relative_path: ValidatedRelativePath,
    pub expected_sha256: Sha256Digest,
    pub policy: ReadPolicy,
    expected_length: u64,
    ancestor_handles: Vec<Arc<fs::File>>,
    source_handle: Arc<fs::File>,
}

impl PartialEq for PlannedRead {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot
            && self.identifier == other.identifier
            && self.relative_path == other.relative_path
            && self.expected_sha256 == other.expected_sha256
            && self.policy == other.policy
            && self.expected_length == other.expected_length
            && self.ancestor_handles.len() == other.ancestor_handles.len()
            && self
                .ancestor_handles
                .iter()
                .zip(&other.ancestor_handles)
                .all(|(left, right)| Arc::ptr_eq(left, right))
            && Arc::ptr_eq(&self.source_handle, &other.source_handle)
    }
}

impl Eq for PlannedRead {}

impl fmt::Debug for PlannedRead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlannedRead")
            .field("slot", &self.slot)
            .field("identifier", &self.identifier)
            .field("relative_path", &"<redacted>")
            .field("expected_sha256", &self.expected_sha256)
            .field("policy", &self.policy)
            .field("expected_length", &self.expected_length)
            .field("filesystem_identity", &"<retained-handle>")
            .finish()
    }
}

impl PlannedRead {
    #[must_use]
    pub const fn expected_length(&self) -> u64 {
        self.expected_length
    }

    pub fn try_clone_source_handle(&self) -> io::Result<fs::File> {
        self.source_handle.try_clone()
    }

    pub fn try_clone_ancestor_handles(&self) -> io::Result<Vec<fs::File>> {
        self.ancestor_handles
            .iter()
            .map(|handle| handle.try_clone())
            .collect()
    }

    #[must_use]
    pub fn source_ancestor_count(&self) -> usize {
        self.ancestor_handles.len()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum OutputArtifact {
    #[serde(rename = "theme.json")]
    ThemeJson,
    #[serde(rename = "background.png")]
    BackgroundPng,
    #[serde(rename = "music.ogg")]
    MusicOgg,
    #[serde(rename = "music.wav")]
    MusicWav,
    #[serde(rename = "provenance.json")]
    ProvenanceJson,
}

impl OutputArtifact {
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::ThemeJson => "theme.json",
            Self::BackgroundPng => "background.png",
            Self::MusicOgg => "music.ogg",
            Self::MusicWav => "music.wav",
            Self::ProvenanceJson => "provenance.json",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct PlannedOutput {
    pub artifact: OutputArtifact,
    pub relative_path: ValidatedRelativePath,
}

impl fmt::Debug for PlannedOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlannedOutput")
            .field("artifact", &self.artifact)
            .field("relative_path", &"<redacted>")
            .finish()
    }
}

/// A host-only plan that cannot cross a serde boundary.
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<deltamod_theme_recipes::ExecutionPlan>();
/// ```
#[derive(Clone)]
pub struct ExecutionPlan {
    root: SelectedGameRoot,
    recipe: ValidatedRecipe,
    selectors: ValidatedSelectors,
    reads: [PlannedRead; 2],
    outputs: [PlannedOutput; 4],
    theme_manifest: ThemeManifest,
}

impl fmt::Debug for ExecutionPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output_artifacts = self.outputs.each_ref().map(|output| output.artifact);
        formatter
            .debug_struct("ExecutionPlan")
            .field("source_root", &"<redacted>")
            .field("source_root_identity", &"<retained-handle>")
            .field("recipe_id", &self.recipe.id())
            .field("selectors", &"<redacted>")
            .field("reads", &"<redacted>")
            .field("output_artifacts", &output_artifacts)
            .finish()
    }
}

impl ExecutionPlan {
    /// The runtime may use this root only with the no-follow policy carried by each read.
    #[must_use]
    pub fn canonical_game_root(&self) -> &Path {
        self.root.canonical_path()
    }

    pub fn try_clone_source_root_handle(&self) -> io::Result<fs::File> {
        self.root.handle().try_clone()
    }

    #[must_use]
    pub const fn recipe(&self) -> &ValidatedRecipe {
        &self.recipe
    }

    #[must_use]
    pub const fn selectors(&self) -> &ValidatedSelectors {
        &self.selectors
    }

    #[must_use]
    pub const fn reads(&self) -> &[PlannedRead; 2] {
        &self.reads
    }

    #[must_use]
    pub const fn outputs(&self) -> &[PlannedOutput; 4] {
        &self.outputs
    }

    #[must_use]
    pub const fn theme_manifest(&self) -> &ThemeManifest {
        &self.theme_manifest
    }

    pub fn theme_json(&self) -> Result<Vec<u8>> {
        pretty_json(self.theme_manifest())
    }

    pub fn validate_media_reader(
        &self,
        slot: SourceSlot,
        reader: &mut dyn Read,
        length: u64,
    ) -> Result<Sha256Digest> {
        let media_type = match slot {
            SourceSlot::Background => MediaType::Png,
            SourceSlot::Music => match self.recipe.definition().output.audio_format {
                AudioFormat::Ogg => MediaType::Ogg,
                AudioFormat::Wav => MediaType::Wav,
            },
        };
        validate_opened_media(reader, length, media_type).map(|(_, digest)| digest)
    }

    #[must_use]
    pub fn transforms(&self, slot: SourceSlot) -> &[Transform] {
        match slot {
            SourceSlot::Background => &self.recipe.definition().transforms.background,
            SourceSlot::Music => &self.recipe.definition().transforms.music,
        }
    }
}

pub fn build_execution_plan(
    recipe: &ValidatedRecipe,
    root: &SelectedGameRoot,
    selectors: Option<&ValidatedSelectors>,
    attestation: RightsAttestation,
) -> Result<ExecutionPlan> {
    if !attestation.is_accepted() {
        return Err(RecipeError::RightsAttestationRequired);
    }
    let selectors = selectors.ok_or(RecipeError::UnresolvedSelectors)?;
    if !selectors.matches(root, recipe) {
        return Err(RecipeError::SelectorContextMismatch);
    }

    let background = selectors.background();
    let music = selectors.music();
    let reads = [
        PlannedRead {
            slot: background.slot(),
            identifier: background.identifier().clone(),
            relative_path: background.relative_path().clone(),
            expected_sha256: background.sha256().clone(),
            policy: ReadPolicy::NoFollow,
            expected_length: background.length(),
            ancestor_handles: background.ancestor_handles().to_vec(),
            source_handle: Arc::clone(background.source_handle()),
        },
        PlannedRead {
            slot: music.slot(),
            identifier: music.identifier().clone(),
            relative_path: music.relative_path().clone(),
            expected_sha256: music.sha256().clone(),
            policy: ReadPolicy::NoFollow,
            expected_length: music.length(),
            ancestor_handles: music.ancestor_handles().to_vec(),
            source_handle: Arc::clone(music.source_handle()),
        },
    ];

    let music_artifact = match recipe.definition().output.audio_format {
        AudioFormat::Ogg => OutputArtifact::MusicOgg,
        AudioFormat::Wav => OutputArtifact::MusicWav,
    };
    let artifacts = [
        OutputArtifact::ThemeJson,
        OutputArtifact::BackgroundPng,
        music_artifact,
        OutputArtifact::ProvenanceJson,
    ];
    let outputs = artifacts
        .map(|artifact| planned_output(recipe.id(), artifact))
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| RecipeError::InvalidOutputPath)?;

    let theme_manifest = ThemeManifest {
        schema_version: THEME_MANIFEST_SCHEMA_VERSION,
        id: recipe.id(),
        name: recipe.definition().name.clone(),
        description: format!(
            "Generated locally from a user-selected {} installation.",
            recipe.definition().attribution.source_work
        ),
        built_in: false,
        icon: Some(OutputArtifact::BackgroundPng.file_name().to_owned()),
        music: Some(music_artifact.file_name().to_owned()),
        local_only: true,
    };

    Ok(ExecutionPlan {
        root: root.clone(),
        recipe: recipe.clone(),
        selectors: selectors.clone(),
        reads,
        outputs,
        theme_manifest,
    })
}

fn planned_output(id: crate::RecipeId, artifact: OutputArtifact) -> Result<PlannedOutput> {
    let relative = format!("customThemes/{}/{}", id.as_str(), artifact.file_name());
    let relative_path =
        ValidatedRelativePath::parse(&relative).map_err(|_| RecipeError::InvalidOutputPath)?;
    Ok(PlannedOutput {
        artifact,
        relative_path,
    })
}

pub(crate) fn pretty_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec_pretty(value).map_err(RecipeError::Serialization)?;
    json.push(b'\n');
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_file_names_are_closed() {
        assert_eq!(OutputArtifact::ThemeJson.file_name(), "theme.json");
        assert_eq!(OutputArtifact::BackgroundPng.file_name(), "background.png");
        assert_eq!(OutputArtifact::MusicOgg.file_name(), "music.ogg");
        assert_eq!(OutputArtifact::MusicWav.file_name(), "music.wav");
        assert_eq!(
            OutputArtifact::ProvenanceJson.file_name(),
            "provenance.json"
        );
    }
}
