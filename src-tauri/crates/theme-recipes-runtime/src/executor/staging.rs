use super::{
    error::{io_failure, ErrorCode, ErrorReport, ExecutionPhase, Result},
    fs_guard::{GuardedRoot, OpenedSource},
    valid_identity, CancellationToken, Checkpoint, LocalThemeRecipeExecutor, StagingRequest,
};
use deltamod_mods_themes_runtime::ThemeJson;
use deltamod_theme_recipes::{
    build_provenance, provenance_json, AudioFormat, ExecutionPlan, OutputArtifact, OutputScope,
    ProvenanceDocument, ReadPolicy, RecipeId, RightsMarker, Sha256Digest, SourceSlot,
    ThemeManifest, Transform, TransformKind, ValidatedRecipe, EXTRACTOR_VERSION, MAX_SOURCE_BYTES,
    PROVENANCE_SCHEMA_VERSION, THEME_MANIFEST_SCHEMA_VERSION,
};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::{collections::BTreeSet, fmt};

pub const STAGING_INTENT_FILE: &str = "theme-recipe-staging-intent.json";
pub const PUBLICATION_HANDOFF_FILE: &str = "theme-recipe-publication-handoff.json";

const STAGING_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_JSON_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationState {
    Unwired,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StagedOutput {
    artifact: OutputArtifact,
    staging_file: String,
    destination: String,
    sha256: Sha256Digest,
    length: u64,
}

impl fmt::Debug for StagedOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagedOutput")
            .field("artifact", &self.artifact)
            .field("staging_file", &self.staging_file)
            .field("destination", &self.destination)
            .field("sha256", &self.sha256)
            .field("length", &self.length)
            .finish()
    }
}

impl StagedOutput {
    #[must_use]
    pub const fn artifact(&self) -> OutputArtifact {
        self.artifact
    }

    #[must_use]
    pub fn staging_file(&self) -> &str {
        &self.staging_file
    }

    #[must_use]
    pub fn destination(&self) -> &str {
        &self.destination
    }

    #[must_use]
    pub const fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationHandoff {
    schema_version: u32,
    operation_id: String,
    idempotency_key: String,
    request_fingerprint: Sha256Digest,
    staging_root_binding: Sha256Digest,
    recipe_id: RecipeId,
    rights: RightsMarker,
    publication: PublicationState,
    outputs: [StagedOutput; 4],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPublicationHandoff {
    schema_version: u32,
    operation_id: String,
    idempotency_key: String,
    request_fingerprint: Sha256Digest,
    staging_root_binding: Sha256Digest,
    recipe_id: RecipeId,
    rights: RightsMarker,
    publication: PublicationState,
    outputs: [StagedOutput; 4],
}

impl<'de> Deserialize<'de> for PublicationHandoff {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPublicationHandoff::deserialize(deserializer)?;
        let handoff = Self {
            schema_version: raw.schema_version,
            operation_id: raw.operation_id,
            idempotency_key: raw.idempotency_key,
            request_fingerprint: raw.request_fingerprint,
            staging_root_binding: raw.staging_root_binding,
            recipe_id: raw.recipe_id,
            rights: raw.rights,
            publication: raw.publication,
            outputs: raw.outputs,
        };
        handoff.validate_closed().map_err(de::Error::custom)?;
        Ok(handoff)
    }
}

impl fmt::Debug for PublicationHandoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicationHandoff")
            .field("schema_version", &self.schema_version)
            .field("operation_id", &self.operation_id)
            .field("idempotency_key", &"<redacted>")
            .field("request_fingerprint", &self.request_fingerprint)
            .field("staging_root_binding", &self.staging_root_binding)
            .field("recipe_id", &self.recipe_id)
            .field("rights", &self.rights)
            .field("publication", &self.publication)
            .field("outputs", &self.outputs)
            .finish()
    }
}

impl PublicationHandoff {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    #[must_use]
    pub const fn request_fingerprint(&self) -> &Sha256Digest {
        &self.request_fingerprint
    }

    #[must_use]
    pub const fn staging_root_binding(&self) -> &Sha256Digest {
        &self.staging_root_binding
    }

    #[must_use]
    pub const fn recipe_id(&self) -> RecipeId {
        self.recipe_id
    }

    #[must_use]
    pub const fn rights(&self) -> RightsMarker {
        self.rights
    }

    #[must_use]
    pub const fn publication(&self) -> PublicationState {
        self.publication
    }

    #[must_use]
    pub const fn outputs(&self) -> &[StagedOutput; 4] {
        &self.outputs
    }

    fn validate_closed(&self) -> std::result::Result<(), &'static str> {
        if self.schema_version != STAGING_SCHEMA_VERSION
            || !valid_identity(&self.operation_id)
            || !valid_identity(&self.idempotency_key)
            || self.rights != RightsMarker::UserAuthorizedInstallationLocalOnly
            || self.publication != PublicationState::Unwired
        {
            return Err("invalid publication handoff identity");
        }
        let audio = self.outputs[2].artifact;
        if !matches!(audio, OutputArtifact::MusicOgg | OutputArtifact::MusicWav) {
            return Err("invalid publication handoff audio artifact");
        }
        let expected = [
            OutputArtifact::ThemeJson,
            OutputArtifact::BackgroundPng,
            audio,
            OutputArtifact::ProvenanceJson,
        ];
        for (output, artifact) in self.outputs.iter().zip(expected) {
            if output.artifact != artifact
                || output.staging_file != artifact.file_name()
                || output.destination
                    != format!(
                        "customThemes/{}/{}",
                        self.recipe_id.as_str(),
                        artifact.file_name()
                    )
                || output.length == 0
            {
                return Err("invalid publication handoff output");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StagingIntent {
    schema_version: u32,
    operation_id: String,
    idempotency_key: String,
    request_fingerprint: Sha256Digest,
    staging_root_binding: Sha256Digest,
    recipe_id: RecipeId,
    rights: RightsMarker,
}

pub(crate) fn execute(
    executor: &LocalThemeRecipeExecutor,
    request: &StagingRequest,
    plan: &ExecutionPlan,
    cancellation: &CancellationToken,
) -> Result<PublicationHandoff> {
    let root = executor.root();
    root.check(ExecutionPhase::ValidateInput)?;
    let shape = PlanShape::from_plan(plan)?;
    let request_fingerprint = plan_fingerprint(plan, &shape);
    let staging_root_binding = root.binding_sha256();
    let expected_handoff = PublicationHandoff {
        schema_version: STAGING_SCHEMA_VERSION,
        operation_id: request.operation_id().to_owned(),
        idempotency_key: request.idempotency_key().to_owned(),
        request_fingerprint: request_fingerprint.clone(),
        staging_root_binding: staging_root_binding.clone(),
        recipe_id: shape.id,
        rights: RightsMarker::UserAuthorizedInstallationLocalOnly,
        publication: PublicationState::Unwired,
        outputs: shape.staged_outputs(plan)?,
    };
    expected_handoff
        .validate_closed()
        .map_err(|_| ErrorReport::new(ErrorCode::InvalidPlan, ExecutionPhase::ValidateInput))?;

    let names = entry_names(root, ExecutionPhase::ValidateInput)?;
    if names.contains(PUBLICATION_HANDOFF_FILE) {
        return load_completed(root, plan, &shape, &expected_handoff);
    }

    let expected_intent = StagingIntent {
        schema_version: STAGING_SCHEMA_VERSION,
        operation_id: request.operation_id().to_owned(),
        idempotency_key: request.idempotency_key().to_owned(),
        request_fingerprint,
        staging_root_binding,
        recipe_id: shape.id,
        rights: RightsMarker::UserAuthorizedInstallationLocalOnly,
    };

    if names.is_empty() {
        executor.checkpoint(
            Checkpoint::BeforeIntent,
            ExecutionPhase::BindIntent,
            cancellation,
        )?;
        root.write_new_bytes(
            STAGING_INTENT_FILE,
            &canonical_json(&expected_intent, ErrorCode::InvalidPlan)?,
            ExecutionPhase::BindIntent,
        )?;
    } else {
        validate_partial_names(&names, &expected_handoff)?;
    }
    let actual_intent = read_intent(root)?;
    if actual_intent != expected_intent {
        return Err(ErrorReport::new(
            ErrorCode::IdempotencyConflict,
            ExecutionPhase::BindIntent,
        ));
    }
    executor.checkpoint(
        Checkpoint::AfterIntent,
        ExecutionPhase::BindIntent,
        cancellation,
    )?;
    executor.checkpoint(
        Checkpoint::LockHeld,
        ExecutionPhase::AcquireLock,
        cancellation,
    )?;

    for output in &expected_handoff.outputs {
        if root.child_exists(output.staging_file(), ExecutionPhase::Stage)? {
            validate_output(root, plan, &shape, output, ExecutionPhase::Verify)?;
            continue;
        }
        executor.checkpoint(
            Checkpoint::BeforeOutput(output.artifact()),
            ExecutionPhase::Stage,
            cancellation,
        )?;
        stage_output(root, plan, &shape, output)?;
    }

    executor.checkpoint(
        Checkpoint::BeforeVerification,
        ExecutionPhase::Verify,
        cancellation,
    )?;
    validate_staged_set(root, plan, &shape, &expected_handoff, false)?;
    executor.checkpoint(
        Checkpoint::BeforeHandoff,
        ExecutionPhase::PersistHandoff,
        cancellation,
    )?;
    root.write_new_bytes(
        PUBLICATION_HANDOFF_FILE,
        &canonical_json(&expected_handoff, ErrorCode::InvalidStagedPackage)?,
        ExecutionPhase::PersistHandoff,
    )?;
    load_completed(root, plan, &shape, &expected_handoff)
}

fn load_completed(
    root: &GuardedRoot,
    plan: &ExecutionPlan,
    shape: &PlanShape,
    expected: &PublicationHandoff,
) -> Result<PublicationHandoff> {
    let actual_intent = read_intent(root)?;
    if actual_intent.operation_id != expected.operation_id
        || actual_intent.idempotency_key != expected.idempotency_key
        || actual_intent.request_fingerprint != expected.request_fingerprint
        || actual_intent.staging_root_binding != expected.staging_root_binding
        || actual_intent.recipe_id != expected.recipe_id
        || actual_intent.rights != expected.rights
    {
        return Err(ErrorReport::new(
            ErrorCode::IdempotencyConflict,
            ExecutionPhase::ReadHandoff,
        ));
    }
    let bytes = root.read_bounded(
        PUBLICATION_HANDOFF_FILE,
        MAX_MANIFEST_BYTES,
        ErrorCode::InvalidStagedPackage,
        ExecutionPhase::ReadHandoff,
    )?;
    let actual: PublicationHandoff = serde_json::from_slice(&bytes).map_err(|_| {
        ErrorReport::new(ErrorCode::InvalidStagedPackage, ExecutionPhase::ReadHandoff)
    })?;
    if canonical_json(&actual, ErrorCode::InvalidStagedPackage)? != bytes {
        return Err(ErrorReport::new(
            ErrorCode::InvalidStagedPackage,
            ExecutionPhase::ReadHandoff,
        ));
    }
    if &actual != expected {
        return Err(ErrorReport::new(
            ErrorCode::IdempotencyConflict,
            ExecutionPhase::ReadHandoff,
        ));
    }
    validate_staged_set(root, plan, shape, &actual, true)?;
    Ok(actual)
}

fn read_intent(root: &GuardedRoot) -> Result<StagingIntent> {
    let bytes = root.read_bounded(
        STAGING_INTENT_FILE,
        MAX_MANIFEST_BYTES,
        ErrorCode::IncompleteStaging,
        ExecutionPhase::BindIntent,
    )?;
    let intent: StagingIntent = serde_json::from_slice(&bytes)
        .map_err(|_| ErrorReport::new(ErrorCode::IncompleteStaging, ExecutionPhase::BindIntent))?;
    if intent.schema_version != STAGING_SCHEMA_VERSION
        || !valid_identity(&intent.operation_id)
        || !valid_identity(&intent.idempotency_key)
        || intent.rights != RightsMarker::UserAuthorizedInstallationLocalOnly
        || canonical_json(&intent, ErrorCode::IncompleteStaging)? != bytes
    {
        return Err(ErrorReport::new(
            ErrorCode::IncompleteStaging,
            ExecutionPhase::BindIntent,
        ));
    }
    Ok(intent)
}

fn validate_partial_names(names: &BTreeSet<String>, handoff: &PublicationHandoff) -> Result<()> {
    if !names.contains(STAGING_INTENT_FILE) || names.contains(PUBLICATION_HANDOFF_FILE) {
        return Err(ErrorReport::new(
            ErrorCode::IncompleteStaging,
            ExecutionPhase::BindIntent,
        ));
    }
    let mut allowed = BTreeSet::from([STAGING_INTENT_FILE.to_owned()]);
    allowed.extend(
        handoff
            .outputs
            .iter()
            .map(|output| output.staging_file.clone()),
    );
    if names.is_subset(&allowed) {
        Ok(())
    } else {
        Err(ErrorReport::new(
            ErrorCode::IncompleteStaging,
            ExecutionPhase::BindIntent,
        ))
    }
}

fn validate_staged_set(
    root: &GuardedRoot,
    plan: &ExecutionPlan,
    shape: &PlanShape,
    handoff: &PublicationHandoff,
    include_handoff: bool,
) -> Result<()> {
    root.check(ExecutionPhase::Verify)?;
    let mut expected = BTreeSet::from([STAGING_INTENT_FILE.to_owned()]);
    expected.extend(
        handoff
            .outputs
            .iter()
            .map(|output| output.staging_file.clone()),
    );
    if include_handoff {
        expected.insert(PUBLICATION_HANDOFF_FILE.to_owned());
    }
    if entry_names(root, ExecutionPhase::Verify)? != expected {
        return Err(ErrorReport::new(
            ErrorCode::InvalidStagedPackage,
            ExecutionPhase::Verify,
        ));
    }
    for output in &handoff.outputs {
        validate_output(root, plan, shape, output, ExecutionPhase::Verify)?;
    }
    root.check(ExecutionPhase::Verify)
}

fn entry_names(root: &GuardedRoot, phase: ExecutionPhase) -> Result<BTreeSet<String>> {
    root.names(phase)?
        .into_iter()
        .map(|name| {
            name.into_string()
                .map_err(|_| ErrorReport::new(ErrorCode::IncompleteStaging, phase))
        })
        .collect()
}

fn stage_output(
    root: &GuardedRoot,
    plan: &ExecutionPlan,
    shape: &PlanShape,
    output: &StagedOutput,
) -> Result<()> {
    match output.artifact {
        OutputArtifact::ThemeJson => root.write_new_bytes(
            output.staging_file(),
            &shape.theme_json,
            ExecutionPhase::Stage,
        )?,
        OutputArtifact::ProvenanceJson => root.write_new_bytes(
            output.staging_file(),
            &shape.provenance_json,
            ExecutionPhase::Stage,
        )?,
        OutputArtifact::BackgroundPng => {
            copy_selected_source(plan, root, SourceSlot::Background, output.staging_file())?
        }
        OutputArtifact::MusicOgg | OutputArtifact::MusicWav => {
            copy_selected_source(plan, root, SourceSlot::Music, output.staging_file())?;
        }
    }
    validate_output(root, plan, shape, output, ExecutionPhase::Verify)
}

fn copy_selected_source(
    plan: &ExecutionPlan,
    root: &GuardedRoot,
    slot: SourceSlot,
    destination_name: &str,
) -> Result<()> {
    let phase = ExecutionPhase::Stage;
    let read = plan
        .reads()
        .iter()
        .find(|read| read.slot == slot)
        .ok_or_else(|| ErrorReport::new(ErrorCode::InvalidPlan, phase))?;
    let mut source = OpenedSource::open(
        plan.canonical_game_root(),
        io_failure(plan.try_clone_source_root_handle(), phase)?,
        io_failure(read.try_clone_ancestor_handles(), phase)?,
        read.relative_path.as_str(),
        io_failure(read.try_clone_source_handle(), phase)?,
        read.expected_length(),
        phase,
    )?;
    let mut output = root.create_new_file(destination_name, phase)?;
    let digest = source.copy_hashing(&mut output, phase)?;
    io_failure(output.sync_all(), phase)?;
    source.finish(phase)?;
    drop(output);
    root.check(phase)?;
    if hex_digest(digest) != read.expected_sha256.as_str() {
        return Err(ErrorReport::new(ErrorCode::SourceChanged, phase));
    }
    Ok(())
}

fn validate_output(
    root: &GuardedRoot,
    plan: &ExecutionPlan,
    shape: &PlanShape,
    output: &StagedOutput,
    phase: ExecutionPhase,
) -> Result<()> {
    let actual = match output.artifact {
        OutputArtifact::ThemeJson => {
            let bytes = root.read_bounded(
                output.staging_file(),
                MAX_JSON_BYTES,
                ErrorCode::InvalidStagedPackage,
                phase,
            )?;
            if bytes != shape.theme_json {
                return Err(ErrorReport::new(ErrorCode::JsonContractMismatch, phase));
            }
            parse_theme_contract(
                &bytes,
                &shape.recipe,
                shape.audio_artifact,
                Some(&shape.theme_manifest),
            )?;
            (Sha256Digest::from_bytes(&bytes), bytes.len() as u64)
        }
        OutputArtifact::ProvenanceJson => {
            let bytes = root.read_bounded(
                output.staging_file(),
                MAX_JSON_BYTES,
                ErrorCode::InvalidStagedPackage,
                phase,
            )?;
            if bytes != shape.provenance_json {
                return Err(ErrorReport::new(ErrorCode::ProvenanceMismatch, phase));
            }
            let document: ProvenanceDocument = serde_json::from_slice(&bytes)
                .map_err(|_| ErrorReport::new(ErrorCode::ProvenanceMismatch, phase))?;
            validate_provenance_document(&document, &bytes, &shape.recipe, shape.audio_artifact)?;
            (Sha256Digest::from_bytes(&bytes), bytes.len() as u64)
        }
        OutputArtifact::BackgroundPng | OutputArtifact::MusicOgg | OutputArtifact::MusicWav => {
            let slot = if output.artifact == OutputArtifact::BackgroundPng {
                SourceSlot::Background
            } else {
                SourceSlot::Music
            };
            let mut opened = root.open_regular_file(
                output.staging_file(),
                None,
                ErrorCode::InvalidStagedPackage,
                phase,
            )?;
            let evidence = opened.evidence();
            if evidence.length > MAX_SOURCE_BYTES {
                return Err(ErrorReport::new(ErrorCode::InvalidStagedPackage, phase));
            }
            let digest = plan
                .validate_media_reader(slot, opened.reader(), evidence.length)
                .map_err(|_| ErrorReport::new(ErrorCode::InvalidStagedPackage, phase))?;
            opened.finish(
                root,
                output.staging_file(),
                ErrorCode::InvalidStagedPackage,
                phase,
            )?;
            (digest, evidence.length)
        }
    };
    if actual.0 != output.sha256 || actual.1 != output.length {
        return Err(ErrorReport::new(ErrorCode::InvalidStagedPackage, phase));
    }
    Ok(())
}

struct PlanShape {
    id: RecipeId,
    recipe: ValidatedRecipe,
    audio_artifact: OutputArtifact,
    background_sha256: Sha256Digest,
    background_length: u64,
    music_sha256: Sha256Digest,
    music_length: u64,
    theme_json: Vec<u8>,
    theme_manifest: ThemeManifest,
    provenance_json: Vec<u8>,
}

impl PlanShape {
    fn from_plan(plan: &ExecutionPlan) -> Result<Self> {
        let phase = ExecutionPhase::ValidateInput;
        if !plan.canonical_game_root().is_absolute()
            || plan.recipe().definition().output.scope != OutputScope::CustomThemes
            || plan.reads().iter().any(|read| {
                read.policy != ReadPolicy::NoFollow
                    || read.source_ancestor_count() + 1
                        != read.relative_path.as_str().split('/').count()
            })
        {
            return Err(ErrorReport::new(ErrorCode::InvalidPlan, phase));
        }

        let background = plan
            .reads()
            .iter()
            .find(|read| read.slot == SourceSlot::Background)
            .ok_or_else(|| ErrorReport::new(ErrorCode::InvalidPlan, phase))?;
        let music = plan
            .reads()
            .iter()
            .find(|read| read.slot == SourceSlot::Music)
            .ok_or_else(|| ErrorReport::new(ErrorCode::InvalidPlan, phase))?;
        if background.relative_path != *plan.selectors().background().relative_path()
            || music.relative_path != *plan.selectors().music().relative_path()
            || background.expected_sha256 != *plan.selectors().background().sha256()
            || music.expected_sha256 != *plan.selectors().music().sha256()
            || background.expected_length() != plan.selectors().background().length()
            || music.expected_length() != plan.selectors().music().length()
            || background.identifier != *plan.selectors().background().identifier()
            || music.identifier != *plan.selectors().music().identifier()
            || plan.transforms(SourceSlot::Background)
                != [Transform {
                    kind: TransformKind::CopyVerified,
                }]
            || plan.transforms(SourceSlot::Music)
                != [Transform {
                    kind: TransformKind::CopyVerified,
                }]
        {
            return Err(ErrorReport::new(ErrorCode::InvalidPlan, phase));
        }

        let audio_artifact = match plan.recipe().definition().output.audio_format {
            AudioFormat::Ogg => OutputArtifact::MusicOgg,
            AudioFormat::Wav => OutputArtifact::MusicWav,
        };
        let expected_artifacts = [
            OutputArtifact::ThemeJson,
            OutputArtifact::BackgroundPng,
            audio_artifact,
            OutputArtifact::ProvenanceJson,
        ];
        for (output, artifact) in plan.outputs().iter().zip(expected_artifacts) {
            if output.artifact != artifact
                || output.relative_path.as_str()
                    != format!(
                        "customThemes/{}/{}",
                        plan.recipe().id().as_str(),
                        artifact.file_name()
                    )
            {
                return Err(ErrorReport::new(ErrorCode::InvalidPlan, phase));
            }
        }

        let manifest = plan.theme_manifest();
        if manifest.schema_version != THEME_MANIFEST_SCHEMA_VERSION
            || manifest.id != plan.recipe().id()
            || manifest.built_in
            || !manifest.local_only
            || manifest.icon.as_deref() != Some(OutputArtifact::BackgroundPng.file_name())
            || manifest.music.as_deref() != Some(audio_artifact.file_name())
        {
            return Err(ErrorReport::new(ErrorCode::InvalidPlan, phase));
        }

        let theme_json = plan
            .theme_json()
            .map_err(|_| ErrorReport::new(ErrorCode::JsonContractMismatch, phase))?;
        parse_theme_contract(&theme_json, plan.recipe(), audio_artifact, Some(manifest))?;
        let provenance = build_provenance(plan)
            .map_err(|_| ErrorReport::new(ErrorCode::ProvenanceMismatch, phase))?;
        let provenance_json = provenance_json(&provenance)
            .map_err(|_| ErrorReport::new(ErrorCode::ProvenanceMismatch, phase))?;
        validate_provenance_document(&provenance, &provenance_json, plan.recipe(), audio_artifact)?;

        Ok(Self {
            id: plan.recipe().id(),
            recipe: plan.recipe().clone(),
            audio_artifact,
            background_sha256: background.expected_sha256.clone(),
            background_length: background.expected_length(),
            music_sha256: music.expected_sha256.clone(),
            music_length: music.expected_length(),
            theme_json,
            theme_manifest: manifest.clone(),
            provenance_json,
        })
    }

    fn staged_outputs(&self, plan: &ExecutionPlan) -> Result<[StagedOutput; 4]> {
        let mut outputs = Vec::with_capacity(4);
        for planned in plan.outputs() {
            let (sha256, length) = match planned.artifact {
                OutputArtifact::ThemeJson => (
                    Sha256Digest::from_bytes(&self.theme_json),
                    self.theme_json.len() as u64,
                ),
                OutputArtifact::BackgroundPng => {
                    (self.background_sha256.clone(), self.background_length)
                }
                OutputArtifact::MusicOgg | OutputArtifact::MusicWav => {
                    (self.music_sha256.clone(), self.music_length)
                }
                OutputArtifact::ProvenanceJson => (
                    Sha256Digest::from_bytes(&self.provenance_json),
                    self.provenance_json.len() as u64,
                ),
            };
            outputs.push(StagedOutput {
                artifact: planned.artifact,
                staging_file: planned.artifact.file_name().to_owned(),
                destination: planned.relative_path.as_str().to_owned(),
                sha256,
                length,
            });
        }
        outputs
            .try_into()
            .map_err(|_| ErrorReport::new(ErrorCode::InvalidPlan, ExecutionPhase::ValidateInput))
    }
}

fn plan_fingerprint(plan: &ExecutionPlan, shape: &PlanShape) -> Sha256Digest {
    fn field(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
        output.extend_from_slice(value);
    }

    let mut material = b"deltamod:theme-recipe-staging-request:v1\0".to_vec();
    field(&mut material, shape.id.as_str().as_bytes());
    for read in plan.reads() {
        let slot = match read.slot {
            SourceSlot::Background => b"background".as_slice(),
            SourceSlot::Music => b"music".as_slice(),
        };
        field(&mut material, slot);
        field(&mut material, read.identifier.as_str().as_bytes());
        field(&mut material, read.relative_path.as_str().as_bytes());
        field(&mut material, read.expected_sha256.as_str().as_bytes());
        field(&mut material, &read.expected_length().to_be_bytes());
    }
    for output in plan.outputs() {
        field(&mut material, output.artifact.file_name().as_bytes());
        field(&mut material, output.relative_path.as_str().as_bytes());
    }
    field(
        &mut material,
        Sha256Digest::from_bytes(&shape.theme_json)
            .as_str()
            .as_bytes(),
    );
    field(
        &mut material,
        Sha256Digest::from_bytes(&shape.provenance_json)
            .as_str()
            .as_bytes(),
    );
    Sha256Digest::from_bytes(&material)
}

fn parse_theme_contract(
    bytes: &[u8],
    recipe: &ValidatedRecipe,
    audio_artifact: OutputArtifact,
    exact: Option<&ThemeManifest>,
) -> Result<()> {
    let phase = ExecutionPhase::Verify;
    let runtime: ThemeJson = serde_json::from_slice(bytes)
        .map_err(|_| ErrorReport::new(ErrorCode::JsonContractMismatch, phase))?;
    let manifest: ThemeManifest = serde_json::from_slice(bytes)
        .map_err(|_| ErrorReport::new(ErrorCode::JsonContractMismatch, phase))?;
    if runtime.id != recipe.id().as_str()
        || runtime.built_in
        || runtime.icon.as_deref() != Some(OutputArtifact::BackgroundPng.file_name())
        || runtime.music.as_deref() != Some(audio_artifact.file_name())
        || manifest.schema_version != THEME_MANIFEST_SCHEMA_VERSION
        || manifest.id != recipe.id()
        || manifest.built_in
        || !manifest.local_only
        || manifest.icon.as_deref() != Some(OutputArtifact::BackgroundPng.file_name())
        || manifest.music.as_deref() != Some(audio_artifact.file_name())
        || exact.is_some_and(|expected| &manifest != expected)
    {
        return Err(ErrorReport::new(ErrorCode::JsonContractMismatch, phase));
    }
    Ok(())
}

fn validate_provenance_document(
    document: &ProvenanceDocument,
    bytes: &[u8],
    recipe: &ValidatedRecipe,
    audio_artifact: OutputArtifact,
) -> Result<()> {
    let phase = ExecutionPhase::Verify;
    let canonical = provenance_json(document)
        .map_err(|_| ErrorReport::new(ErrorCode::ProvenanceMismatch, phase))?;
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| ErrorReport::new(ErrorCode::ProvenanceMismatch, phase))?;
    if canonical != bytes
        || json_contains_path_key(&value)
        || document.schema_version != PROVENANCE_SCHEMA_VERSION
        || document.recipe_id != recipe.id()
        || document.extractor_version != EXTRACTOR_VERSION
        || document.rights != RightsMarker::UserAuthorizedInstallationLocalOnly
        || document.attribution != recipe.definition().attribution
    {
        return Err(ErrorReport::new(ErrorCode::ProvenanceMismatch, phase));
    }

    let expected_slots = [SourceSlot::Background, SourceSlot::Music];
    if document.sources.len() != expected_slots.len()
        || document
            .sources
            .iter()
            .zip(expected_slots)
            .any(|(source, slot)| source.slot != slot)
        || document.transforms.len() != expected_slots.len()
        || document
            .transforms
            .iter()
            .zip(expected_slots)
            .any(|(transforms, slot)| {
                transforms.slot != slot
                    || transforms.steps
                        != [Transform {
                            kind: TransformKind::CopyVerified,
                        }]
            })
    {
        return Err(ErrorReport::new(ErrorCode::ProvenanceMismatch, phase));
    }

    let expected_outputs = [
        OutputArtifact::ThemeJson,
        OutputArtifact::BackgroundPng,
        audio_artifact,
    ];
    if document.outputs.len() != expected_outputs.len()
        || document
            .outputs
            .iter()
            .zip(expected_outputs)
            .any(|(output, artifact)| output.file != artifact)
        || document.sources[0].sha256 != document.outputs[1].sha256
        || document.sources[1].sha256 != document.outputs[2].sha256
    {
        return Err(ErrorReport::new(ErrorCode::ProvenanceMismatch, phase));
    }
    Ok(())
}

fn json_contains_path_key(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(entries) => entries.iter().any(|(key, value)| {
            key.to_ascii_lowercase().contains("path") || json_contains_path_key(value)
        }),
        serde_json::Value::Array(values) => values.iter().any(json_contains_path_key),
        _ => false,
    }
}

fn canonical_json<T: Serialize>(value: &T, code: ErrorCode) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| ErrorReport::new(code, ExecutionPhase::Verify))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ErrorReport::new(code, ExecutionPhase::Verify));
    }
    Ok(bytes)
}

fn hex_digest(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}
