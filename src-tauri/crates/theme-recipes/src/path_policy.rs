use crate::{
    AudioFormat, MediaType, RecipeError, Result, RightsAttestation, SourceSlot, ValidatedRecipe,
};
use deltamod_product_contracts::{PreflightTransactionRoot, ValidatedRelativePath};
use serde::{de, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceIdentifier(String);

impl SourceIdentifier {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || matches!(value, "." | "..")
        {
            return Err(RecipeError::InvalidSourceIdentifier);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SourceIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SourceIdentifier")
            .field(&self.0)
            .finish()
    }
}

impl Serialize for SourceIdentifier {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SourceIdentifier {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(RecipeError::InvalidSha256);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_hash(Sha256::digest(bytes).into())
    }

    fn from_hash(hash: [u8; 32]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = String::with_capacity(64);
        for byte in hash {
            value.push(char::from(HEX[usize::from(byte >> 4)]));
            value.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Sha256Digest")
            .field(&self.0)
            .finish()
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

/// Host-local selector input.
///
/// Deserialization accepts the validated source-relative path needed by the trusted selector,
/// while diagnostic serialization deliberately emits a redaction marker. It is not a round-trip
/// wire contract and must not be used to move source paths across IPC.
#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectorCandidate {
    pub slot: SourceSlot,
    pub identifier: SourceIdentifier,
    pub relative_path: ValidatedRelativePath,
}

impl fmt::Debug for SelectorCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectorCandidate")
            .field("slot", &self.slot)
            .field("identifier", &self.identifier)
            .field("relative_path", &"<redacted>")
            .finish()
    }
}

impl Serialize for SelectorCandidate {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SelectorCandidate", 3)?;
        state.serialize_field("slot", &self.slot)?;
        state.serialize_field("identifier", &self.identifier)?;
        state.serialize_field("relativePath", "<redacted>")?;
        state.end()
    }
}

impl SelectorCandidate {
    pub fn new(slot: SourceSlot, identifier: &str, relative_path: &str) -> Result<Self> {
        Ok(Self {
            slot,
            identifier: SourceIdentifier::parse(identifier)?,
            relative_path: ValidatedRelativePath::parse(relative_path)?,
        })
    }
}

#[derive(Clone)]
pub struct ValidatedSelector {
    slot: SourceSlot,
    identifier: SourceIdentifier,
    relative_path: ValidatedRelativePath,
    sha256: Sha256Digest,
    length: u64,
    ancestor_handles: Vec<Arc<fs::File>>,
    source_handle: Arc<fs::File>,
    media_type: MediaType,
}

impl PartialEq for ValidatedSelector {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot
            && self.identifier == other.identifier
            && self.relative_path == other.relative_path
            && self.sha256 == other.sha256
            && self.length == other.length
            && self.media_type == other.media_type
            && self.ancestor_handles.len() == other.ancestor_handles.len()
            && self
                .ancestor_handles
                .iter()
                .zip(&other.ancestor_handles)
                .all(|(left, right)| Arc::ptr_eq(left, right))
            && Arc::ptr_eq(&self.source_handle, &other.source_handle)
    }
}

impl Eq for ValidatedSelector {}

impl fmt::Debug for ValidatedSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedSelector")
            .field("slot", &self.slot)
            .field("identifier", &self.identifier)
            .field("relative_path", &"<redacted>")
            .field("sha256", &self.sha256)
            .field("length", &self.length)
            .field("filesystem_identity", &"<retained-handle>")
            .field("media_type", &self.media_type)
            .finish()
    }
}

impl ValidatedSelector {
    #[must_use]
    pub const fn slot(&self) -> SourceSlot {
        self.slot
    }

    #[must_use]
    pub const fn identifier(&self) -> &SourceIdentifier {
        &self.identifier
    }

    #[must_use]
    pub const fn relative_path(&self) -> &ValidatedRelativePath {
        &self.relative_path
    }

    #[must_use]
    pub const fn sha256(&self) -> &Sha256Digest {
        &self.sha256
    }

    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub(crate) fn ancestor_handles(&self) -> &[Arc<fs::File>] {
        &self.ancestor_handles
    }

    pub(crate) fn source_handle(&self) -> &Arc<fs::File> {
        &self.source_handle
    }

    #[must_use]
    pub const fn media_type(&self) -> MediaType {
        self.media_type
    }
}

#[derive(Clone)]
pub struct SelectedGameRoot {
    boundary: PreflightTransactionRoot,
    handle: Arc<fs::File>,
}

impl fmt::Debug for SelectedGameRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectedGameRoot")
            .field("canonical_path", &"<redacted>")
            .finish()
    }
}

impl SelectedGameRoot {
    pub fn open_user_selected(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(RecipeError::GameRootMustBeAbsolute);
        }
        let boundary = PreflightTransactionRoot::open(path)?;
        let handle =
            open_no_follow(boundary.canonical_path(), true).map_err(RecipeError::SourceIo)?;
        let metadata = handle.metadata().map_err(RecipeError::SourceIo)?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(RecipeError::UnsafeSource);
        }
        Ok(Self {
            boundary,
            handle: Arc::new(handle),
        })
    }

    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        self.boundary.canonical_path()
    }

    pub(crate) fn handle(&self) -> &Arc<fs::File> {
        &self.handle
    }

    fn check_open_roots(&self) -> Result<()> {
        let metadata = self.handle.metadata().map_err(RecipeError::SourceIo)?;
        let current = open_no_follow(self.canonical_path(), true).map_err(RecipeError::SourceIo)?;
        let current_metadata = current.metadata().map_err(RecipeError::SourceIo)?;
        if !metadata.is_dir()
            || is_link_or_reparse(&metadata)
            || !current_metadata.is_dir()
            || is_link_or_reparse(&current_metadata)
        {
            return Err(RecipeError::UnsafeSource);
        }
        Ok(())
    }

    pub fn validate_selectors(
        &self,
        recipe: &ValidatedRecipe,
        candidates: &[SelectorCandidate],
        attestation: RightsAttestation,
    ) -> Result<ValidatedSelectors> {
        if !attestation.is_accepted() {
            return Err(RecipeError::RightsAttestationRequired);
        }
        self.check_open_roots()?;
        let mut by_slot = BTreeMap::new();
        let mut identifiers = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for candidate in candidates {
            if by_slot.insert(candidate.slot, candidate).is_some()
                || !identifiers.insert(candidate.identifier.clone())
                || !paths.insert(candidate.relative_path.clone())
            {
                return Err(RecipeError::DuplicateSelector);
            }
        }
        for slot in SourceSlot::ALL {
            if !by_slot.contains_key(&slot) {
                return Err(RecipeError::MissingSelector(slot));
            }
        }

        let background = self.inspect_selector(
            by_slot[&SourceSlot::Background],
            ExpectedMedia::Exact(MediaType::Png),
        )?;
        let music_type = match recipe.definition().output.audio_format {
            AudioFormat::Ogg => MediaType::Ogg,
            AudioFormat::Wav => MediaType::Wav,
        };
        let music = self.inspect_selector(
            by_slot[&SourceSlot::Music],
            ExpectedMedia::Exact(music_type),
        )?;

        self.check_open_roots()?;
        Ok(ValidatedSelectors {
            root: self.canonical_path().to_path_buf(),
            root_handle: Arc::clone(&self.handle),
            recipe_id: recipe.id(),
            background,
            music,
        })
    }

    fn inspect_selector(
        &self,
        candidate: &SelectorCandidate,
        expected: ExpectedMedia,
    ) -> Result<ValidatedSelector> {
        let path = self.boundary.inspect(&candidate.relative_path)?;
        let path_metadata = fs::symlink_metadata(&path).map_err(RecipeError::SourceIo)?;
        if !path_metadata.is_file() || is_link_or_reparse(&path_metadata) {
            return Err(RecipeError::UnsafeSource);
        }
        let resolved = fs::canonicalize(&path).map_err(RecipeError::SourceIo)?;
        if !resolved.starts_with(self.canonical_path()) {
            return Err(RecipeError::UnsafeSource);
        }

        let ancestor_handles =
            open_ancestor_handles(self.canonical_path(), candidate.relative_path.as_str())?;
        let media_type = expected.media_type();
        let evidence = hash_no_follow(&path, media_type)?;
        self.check_open_roots()?;

        Ok(ValidatedSelector {
            slot: candidate.slot,
            identifier: candidate.identifier.clone(),
            relative_path: candidate.relative_path.clone(),
            sha256: evidence.sha256,
            length: evidence.length,
            ancestor_handles,
            source_handle: Arc::new(evidence.file),
            media_type,
        })
    }
}

/// Validated source capabilities remain host-only and intentionally cannot be serialized.
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<deltamod_theme_recipes::ValidatedSelectors>();
/// ```
#[derive(Clone)]
pub struct ValidatedSelectors {
    root: PathBuf,
    root_handle: Arc<fs::File>,
    recipe_id: crate::RecipeId,
    background: ValidatedSelector,
    music: ValidatedSelector,
}

impl fmt::Debug for ValidatedSelectors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedSelectors")
            .field("root", &"<redacted>")
            .field("root_identity", &"<retained-handle>")
            .field("recipe_id", &self.recipe_id)
            .field("background", &"<redacted>")
            .field("music", &"<redacted>")
            .finish()
    }
}

impl ValidatedSelectors {
    #[must_use]
    pub const fn background(&self) -> &ValidatedSelector {
        &self.background
    }

    #[must_use]
    pub const fn music(&self) -> &ValidatedSelector {
        &self.music
    }

    pub(crate) fn matches(&self, root: &SelectedGameRoot, recipe: &ValidatedRecipe) -> bool {
        self.root == root.canonical_path()
            && Arc::ptr_eq(&self.root_handle, root.handle())
            && self.recipe_id == recipe.id()
    }
}

#[derive(Clone, Copy)]
enum ExpectedMedia {
    Exact(MediaType),
}

impl ExpectedMedia {
    const fn media_type(self) -> MediaType {
        match self {
            Self::Exact(expected) => expected,
        }
    }
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

struct HashedSource {
    sha256: Sha256Digest,
    length: u64,
    file: fs::File,
}

fn open_ancestor_handles(root: &Path, relative: &str) -> Result<Vec<Arc<fs::File>>> {
    let components = relative.split('/').collect::<Vec<_>>();
    if components.is_empty() {
        return Err(RecipeError::UnsafeSource);
    }

    let mut current = root.to_path_buf();
    let mut handles = Vec::with_capacity(components.len().saturating_sub(1));
    for component in &components[..components.len() - 1] {
        current.push(component);
        let handle = open_no_follow(&current, true).map_err(RecipeError::SourceIo)?;
        let metadata = handle.metadata().map_err(RecipeError::SourceIo)?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(RecipeError::UnsafeSource);
        }
        handles.push(Arc::new(handle));
    }
    Ok(handles)
}

fn hash_no_follow(path: &Path, expected_media: MediaType) -> Result<HashedSource> {
    let file = open_no_follow(path, false).map_err(RecipeError::SourceIo)?;
    let before = file.metadata().map_err(RecipeError::SourceIo)?;
    if !before.is_file() || is_link_or_reparse(&before) {
        return Err(RecipeError::UnsafeSource);
    }
    if before.len() > MAX_SOURCE_BYTES {
        return Err(RecipeError::SourceTooLarge);
    }

    let (mut file, sha256) = validate_opened_media(file, before.len(), expected_media)?;
    let after = file.metadata().map_err(RecipeError::SourceIo)?;
    if after.len() != before.len() || !after.is_file() || is_link_or_reparse(&after) {
        return Err(RecipeError::UnsafeSource);
    }
    file.rewind().map_err(RecipeError::SourceIo)?;
    Ok(HashedSource {
        sha256,
        length: before.len(),
        file,
    })
}

pub(crate) fn validate_opened_media<R: Read>(
    reader: R,
    length: u64,
    expected_media: MediaType,
) -> Result<(R, Sha256Digest)> {
    let mut reader = HashingReader::new(reader);
    validate_media(&mut reader, length, expected_media)?;
    if reader.bytes_read() != length {
        return Err(RecipeError::SourceMediaMismatch);
    }
    let mut extra = [0_u8; 1];
    if reader.read(&mut extra).map_err(RecipeError::SourceIo)? != 0 {
        return Err(RecipeError::UnsafeSource);
    }
    let (reader, hash) = reader.finish();
    Ok((reader, Sha256Digest::from_hash(hash)))
}

struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes_read: u64,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes_read: 0,
        }
    }

    const fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    fn finish(self) -> (R, [u8; 32]) {
        (self.inner, self.hasher.finalize().into())
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.bytes_read >= MAX_SOURCE_BYTES {
            return Ok(0);
        }
        let remaining = usize::try_from(MAX_SOURCE_BYTES - self.bytes_read).unwrap_or(usize::MAX);
        let permitted = buffer.len().min(remaining);
        let read = self.inner.read(&mut buffer[..permitted])?;
        self.bytes_read = self
            .bytes_read
            .checked_add(read as u64)
            .ok_or_else(|| io::Error::other("source byte count overflow"))?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

fn validate_media<R: Read>(
    reader: &mut HashingReader<R>,
    total_length: u64,
    expected_media: MediaType,
) -> Result<()> {
    if total_length > MAX_SOURCE_BYTES {
        return Err(RecipeError::SourceTooLarge);
    }
    match expected_media {
        MediaType::Png => validate_png(reader, total_length),
        MediaType::Ogg => validate_ogg(reader, total_length),
        MediaType::Wav => validate_wave(reader, total_length),
    }
}

fn validate_png<R: Read>(reader: &mut HashingReader<R>, total_length: u64) -> Result<()> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    const MAX_DIMENSION: u32 = 16_384;
    const MAX_PIXELS: u64 = 100_000_000;

    let mut signature = [0_u8; 8];
    read_media_exact(reader, &mut signature)?;
    if &signature != PNG_SIGNATURE {
        return Err(RecipeError::SourceMediaMismatch);
    }

    let mut saw_ihdr = false;
    let mut saw_plte = false;
    let mut saw_idat = false;
    let mut idat_ended = false;
    let mut color_type = 0_u8;
    loop {
        ensure_remaining(total_length, reader.bytes_read(), 12)?;
        let mut length_bytes = [0_u8; 4];
        let mut chunk_type = [0_u8; 4];
        read_media_exact(reader, &mut length_bytes)?;
        read_media_exact(reader, &mut chunk_type)?;
        let chunk_length = u64::from(u32::from_be_bytes(length_bytes));
        ensure_remaining(
            total_length,
            reader.bytes_read(),
            chunk_length
                .checked_add(4)
                .ok_or(RecipeError::SourceMediaMismatch)?,
        )?;
        if !chunk_type.iter().all(u8::is_ascii_alphabetic) || !chunk_type[2].is_ascii_uppercase() {
            return Err(RecipeError::SourceMediaMismatch);
        }
        if !saw_ihdr && &chunk_type != b"IHDR" {
            return Err(RecipeError::SourceMediaMismatch);
        }
        if chunk_type[0].is_ascii_uppercase()
            && !matches!(&chunk_type, b"IHDR" | b"PLTE" | b"IDAT" | b"IEND")
        {
            return Err(RecipeError::SourceMediaMismatch);
        }
        if saw_idat && &chunk_type != b"IDAT" {
            idat_ended = true;
        }
        if idat_ended && &chunk_type == b"IDAT" {
            return Err(RecipeError::SourceMediaMismatch);
        }

        let mut crc = PngCrc::new();
        crc.update(&chunk_type);
        match &chunk_type {
            b"IHDR" => {
                if saw_ihdr || chunk_length != 13 {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                let mut ihdr = [0_u8; 13];
                read_media_exact(reader, &mut ihdr)?;
                crc.update(&ihdr);
                let width = u32::from_be_bytes(ihdr[0..4].try_into().expect("IHDR width"));
                let height = u32::from_be_bytes(ihdr[4..8].try_into().expect("IHDR height"));
                color_type = ihdr[9];
                if width == 0
                    || height == 0
                    || width > MAX_DIMENSION
                    || height > MAX_DIMENSION
                    || u64::from(width) * u64::from(height) > MAX_PIXELS
                    || !valid_png_depth(ihdr[8], color_type)
                    || ihdr[10] != 0
                    || ihdr[11] != 0
                    || !matches!(ihdr[12], 0 | 1)
                {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                saw_ihdr = true;
            }
            b"PLTE" => {
                if !saw_ihdr
                    || saw_plte
                    || saw_idat
                    || chunk_length == 0
                    || chunk_length > 768
                    || chunk_length % 3 != 0
                    || matches!(color_type, 0 | 4)
                {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                consume_media(reader, chunk_length, |bytes| crc.update(bytes))?;
                saw_plte = true;
            }
            b"IDAT" => {
                if !saw_ihdr || chunk_length == 0 || (color_type == 3 && !saw_plte) {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                consume_media(reader, chunk_length, |bytes| crc.update(bytes))?;
                saw_idat = true;
            }
            b"IEND" => {
                if chunk_length != 0 || !saw_idat {
                    return Err(RecipeError::SourceMediaMismatch);
                }
            }
            _ => consume_media(reader, chunk_length, |bytes| crc.update(bytes))?,
        }

        let mut expected_crc = [0_u8; 4];
        read_media_exact(reader, &mut expected_crc)?;
        if crc.finish() != u32::from_be_bytes(expected_crc) {
            return Err(RecipeError::SourceMediaMismatch);
        }
        if &chunk_type == b"IEND" {
            return if reader.bytes_read() == total_length {
                Ok(())
            } else {
                Err(RecipeError::SourceMediaMismatch)
            };
        }
    }
}

fn valid_png_depth(bit_depth: u8, color_type: u8) -> bool {
    matches!(
        (color_type, bit_depth),
        (0, 1 | 2 | 4 | 8 | 16) | (2, 8 | 16) | (3, 1 | 2 | 4 | 8) | (4, 8 | 16) | (6, 8 | 16)
    )
}

struct PngCrc(u32);

impl PngCrc {
    const fn new() -> Self {
        Self(u32::MAX)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u32::from(*byte);
            for _ in 0..8 {
                self.0 = if self.0 & 1 == 0 {
                    self.0 >> 1
                } else {
                    (self.0 >> 1) ^ 0xedb8_8320
                };
            }
        }
    }

    const fn finish(self) -> u32 {
        !self.0
    }
}

fn validate_ogg<R: Read>(reader: &mut HashingReader<R>, total_length: u64) -> Result<()> {
    let mut page_index = 0_u64;
    let mut expected_serial = None;
    let mut expected_sequence = 0_u32;
    let mut expects_continuation = false;
    let mut saw_payload = false;

    while reader.bytes_read() < total_length {
        ensure_remaining(total_length, reader.bytes_read(), 27)?;
        let mut header = [0_u8; 27];
        read_media_exact(reader, &mut header)?;
        if &header[..4] != b"OggS" || header[4] != 0 || header[5] & !0x07 != 0 {
            return Err(RecipeError::SourceMediaMismatch);
        }

        let flags = header[5];
        let continued = flags & 0x01 != 0;
        let beginning = flags & 0x02 != 0;
        let ending = flags & 0x04 != 0;
        if (page_index == 0 && (!beginning || continued))
            || (page_index != 0 && (beginning || continued != expects_continuation))
        {
            return Err(RecipeError::SourceMediaMismatch);
        }

        let serial = u32::from_le_bytes(header[14..18].try_into().expect("Ogg serial"));
        let sequence = u32::from_le_bytes(header[18..22].try_into().expect("Ogg sequence"));
        if expected_serial.is_some_and(|expected| expected != serial)
            || sequence != expected_sequence
        {
            return Err(RecipeError::SourceMediaMismatch);
        }
        expected_serial.get_or_insert(serial);
        expected_sequence = expected_sequence.wrapping_add(1);

        let expected_checksum =
            u32::from_le_bytes(header[22..26].try_into().expect("Ogg checksum"));
        header[22..26].fill(0);
        let mut checksum = OggChecksum::new();
        checksum.update(&header);

        let segment_count = usize::from(header[26]);
        ensure_remaining(
            total_length,
            reader.bytes_read(),
            u64::try_from(segment_count).expect("Ogg segment count fits u64"),
        )?;
        let mut lacing = [0_u8; 255];
        read_media_exact(reader, &mut lacing[..segment_count])?;
        checksum.update(&lacing[..segment_count]);
        let payload_length = lacing[..segment_count]
            .iter()
            .map(|value| u64::from(*value))
            .sum::<u64>();
        ensure_remaining(total_length, reader.bytes_read(), payload_length)?;
        consume_media(reader, payload_length, |bytes| checksum.update(bytes))?;
        if checksum.finish() != expected_checksum {
            return Err(RecipeError::SourceMediaMismatch);
        }

        saw_payload |= payload_length != 0;
        expects_continuation = lacing[..segment_count]
            .last()
            .is_some_and(|value| *value == u8::MAX);
        page_index += 1;

        if ending {
            if !saw_payload || expects_continuation || reader.bytes_read() != total_length {
                return Err(RecipeError::SourceMediaMismatch);
            }
            return Ok(());
        }
    }

    Err(RecipeError::SourceMediaMismatch)
}

struct OggChecksum(u32);

impl OggChecksum {
    const fn new() -> Self {
        Self(0)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u32::from(*byte) << 24;
            for _ in 0..8 {
                self.0 = if self.0 & 0x8000_0000 == 0 {
                    self.0 << 1
                } else {
                    (self.0 << 1) ^ 0x04c1_1db7
                };
            }
        }
    }

    const fn finish(self) -> u32 {
        self.0
    }
}

fn validate_wave<R: Read>(reader: &mut HashingReader<R>, total_length: u64) -> Result<()> {
    let mut header = [0_u8; 12];
    read_media_exact(reader, &mut header)?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(RecipeError::SourceMediaMismatch);
    }
    let declared_length = u64::from(u32::from_le_bytes(
        header[4..8].try_into().expect("RIFF size"),
    ))
    .checked_add(8)
    .ok_or(RecipeError::SourceMediaMismatch)?;
    if declared_length != total_length {
        return Err(RecipeError::SourceMediaMismatch);
    }

    let mut saw_fmt = false;
    let mut saw_data = false;
    let mut block_align = None;
    while reader.bytes_read() < total_length {
        ensure_remaining(total_length, reader.bytes_read(), 8)?;
        let mut chunk_header = [0_u8; 8];
        read_media_exact(reader, &mut chunk_header)?;
        let chunk_length = u64::from(u32::from_le_bytes(
            chunk_header[4..8].try_into().expect("WAVE chunk size"),
        ));
        let padded_length = chunk_length
            .checked_add(chunk_length & 1)
            .ok_or(RecipeError::SourceMediaMismatch)?;
        ensure_remaining(total_length, reader.bytes_read(), padded_length)?;

        match &chunk_header[..4] {
            b"fmt " => {
                if saw_fmt || saw_data || chunk_length < 16 {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                let mut format = [0_u8; 16];
                read_media_exact(reader, &mut format)?;
                block_align = Some(validate_wave_format(&format, chunk_length)?);
                consume_media(reader, chunk_length - 16, |_| {})?;
                saw_fmt = true;
            }
            b"data" => {
                let Some(frame_length) = block_align else {
                    return Err(RecipeError::SourceMediaMismatch);
                };
                if !saw_fmt
                    || saw_data
                    || chunk_length == 0
                    || chunk_length % u64::from(frame_length) != 0
                {
                    return Err(RecipeError::SourceMediaMismatch);
                }
                consume_media(reader, chunk_length, |_| {})?;
                saw_data = true;
            }
            _ => consume_media(reader, chunk_length, |_| {})?,
        }
        if chunk_length & 1 != 0 {
            let mut padding = [0_u8; 1];
            read_media_exact(reader, &mut padding)?;
        }
    }

    if saw_fmt && saw_data {
        Ok(())
    } else {
        Err(RecipeError::SourceMediaMismatch)
    }
}

fn validate_wave_format(format: &[u8; 16], chunk_length: u64) -> Result<u16> {
    let encoding = u16::from_le_bytes(format[0..2].try_into().expect("WAVE encoding"));
    let channels = u16::from_le_bytes(format[2..4].try_into().expect("WAVE channels"));
    let sample_rate = u32::from_le_bytes(format[4..8].try_into().expect("WAVE sample rate"));
    let byte_rate = u32::from_le_bytes(format[8..12].try_into().expect("WAVE byte rate"));
    let block_align = u16::from_le_bytes(format[12..14].try_into().expect("WAVE block align"));
    let bits_per_sample =
        u16::from_le_bytes(format[14..16].try_into().expect("WAVE bits per sample"));

    if !matches!(encoding, 1 | 3 | 0xfffe)
        || (encoding == 0xfffe && chunk_length < 40)
        || !(1..=32).contains(&channels)
        || !(1..=384_000).contains(&sample_rate)
        || bits_per_sample == 0
        || bits_per_sample > 64
        || bits_per_sample % 8 != 0
        || (encoding == 3 && !matches!(bits_per_sample, 32 | 64))
    {
        return Err(RecipeError::SourceMediaMismatch);
    }

    let expected_block_align = u32::from(channels) * u32::from(bits_per_sample / 8);
    let expected_byte_rate = sample_rate
        .checked_mul(expected_block_align)
        .ok_or(RecipeError::SourceMediaMismatch)?;
    if u32::from(block_align) != expected_block_align || byte_rate != expected_byte_rate {
        return Err(RecipeError::SourceMediaMismatch);
    }
    Ok(block_align)
}

fn ensure_remaining(total_length: u64, consumed: u64, required: u64) -> Result<()> {
    if required > total_length.saturating_sub(consumed) {
        return Err(RecipeError::SourceMediaMismatch);
    }
    Ok(())
}

fn read_media_exact(reader: &mut impl Read, buffer: &mut [u8]) -> Result<()> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            Err(RecipeError::SourceMediaMismatch)
        }
        Err(error) => Err(RecipeError::SourceIo(error)),
    }
}

fn consume_media(
    reader: &mut impl Read,
    mut remaining: u64,
    mut inspect: impl FnMut(&[u8]),
) -> Result<()> {
    let mut buffer = [0_u8; 16 * 1024];
    while remaining != 0 {
        let length = usize::try_from(remaining.min(buffer.len() as u64))
            .expect("bounded media read length fits usize");
        read_media_exact(reader, &mut buffer[..length])?;
        inspect(&buffer[..length]);
        remaining -= length as u64;
    }
    Ok(())
}

fn open_no_follow(path: &Path, directory: bool) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                0
            };
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE;
        options.custom_flags(flags).share_mode(share);
    }

    #[cfg(not(windows))]
    let _ = directory;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0o400_000;
        options.custom_flags(O_NOFOLLOW);
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0x100;
        options.custom_flags(O_NOFOLLOW);
    }

    #[cfg(not(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no-follow open is unsupported on this platform",
    ));

    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn digest_parser_normalizes_case() {
        let uppercase = "AA".repeat(32);
        assert_eq!(
            Sha256Digest::parse(&uppercase).unwrap().as_str(),
            "aa".repeat(32)
        );
        assert!(Sha256Digest::parse("not-a-hash").is_err());
    }

    #[test]
    fn structurally_valid_synthetic_media_is_accepted() {
        assert!(validate_bytes(&synthetic_png(), MediaType::Png).is_ok());
        assert!(validate_bytes(&synthetic_ogg(0x06), MediaType::Ogg).is_ok());
        assert!(validate_bytes(&synthetic_wave(1, 8, Some(1)), MediaType::Wav).is_ok());
    }

    #[test]
    fn wave_complete_one_and_multi_frame_controls_are_accepted() {
        for data_length in [4, 12] {
            assert!(
                validate_bytes(&synthetic_wave(2, 16, Some(data_length)), MediaType::Wav).is_ok()
            );
        }
    }

    #[test]
    fn wave_incomplete_stereo_frame_is_rejected() {
        assert_media_mismatch(&synthetic_wave(2, 16, Some(1)), MediaType::Wav);
    }

    #[test]
    fn wave_malformed_padding_and_chunk_lengths_are_rejected() {
        let mut missing_padding = synthetic_wave(1, 8, Some(1));
        missing_padding.pop();
        rewrite_riff_length(&mut missing_padding);
        assert_media_mismatch(&missing_padding, MediaType::Wav);

        let mut forged_data_length = synthetic_wave(2, 16, Some(4));
        forged_data_length[40..44].copy_from_slice(&8_u32.to_le_bytes());
        assert_media_mismatch(&forged_data_length, MediaType::Wav);
    }

    #[test]
    fn magic_prefix_plus_arbitrary_text_is_rejected() {
        assert_media_mismatch(b"\x89PNG\r\n\x1a\nnot-a-png", MediaType::Png);
        assert_media_mismatch(b"OggSnot-an-ogg-page", MediaType::Ogg);
        assert_media_mismatch(b"RIFF\x10\0\0\0WAVEnot-a-wave", MediaType::Wav);
    }

    #[test]
    fn truncated_structures_are_rejected() {
        for (mut bytes, media_type) in [
            (synthetic_png(), MediaType::Png),
            (synthetic_ogg(0x06), MediaType::Ogg),
            (synthetic_wave(1, 8, Some(1)), MediaType::Wav),
        ] {
            bytes.pop();
            assert_media_mismatch(&bytes, media_type);
        }
    }

    #[test]
    fn forged_container_lengths_are_rejected() {
        let mut png = synthetic_png();
        png[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_media_mismatch(&png, MediaType::Png);

        let mut ogg = synthetic_ogg(0x06);
        ogg[27] = 254;
        assert_media_mismatch(&ogg, MediaType::Ogg);

        let mut wave = synthetic_wave(1, 8, Some(1));
        wave[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_media_mismatch(&wave, MediaType::Wav);
    }

    #[test]
    fn missing_terminal_chunks_and_data_are_rejected() {
        let mut png = synthetic_png();
        png.truncate(png.len() - 12);
        assert_media_mismatch(&png, MediaType::Png);

        assert_media_mismatch(&synthetic_ogg(0x02), MediaType::Ogg);
        assert_media_mismatch(&synthetic_wave(1, 8, None), MediaType::Wav);
    }

    #[test]
    fn checksum_corruption_is_rejected() {
        let mut png = synthetic_png();
        png[45] ^= 0x01;
        assert_media_mismatch(&png, MediaType::Png);

        let mut ogg = synthetic_ogg(0x06);
        let last = ogg.len() - 1;
        ogg[last] ^= 0x01;
        assert_media_mismatch(&ogg, MediaType::Ogg);
    }

    fn validate_bytes(bytes: &[u8], media_type: MediaType) -> Result<()> {
        let mut reader = HashingReader::new(Cursor::new(bytes));
        validate_media(&mut reader, bytes.len() as u64, media_type)?;
        if reader.bytes_read() == bytes.len() as u64 {
            Ok(())
        } else {
            Err(RecipeError::SourceMediaMismatch)
        }
    }

    fn assert_media_mismatch(bytes: &[u8], media_type: MediaType) {
        assert!(matches!(
            validate_bytes(bytes, media_type),
            Err(RecipeError::SourceMediaMismatch)
        ));
    }

    fn synthetic_png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        append_png_chunk(&mut bytes, b"IHDR", &ihdr);

        let scanline = [0_u8, 0x21, 0x43, 0x65, 0xff];
        let mut compressed = vec![0x78, 0x01, 0x01];
        let length = u16::try_from(scanline.len()).expect("small PNG scanline");
        compressed.extend_from_slice(&length.to_le_bytes());
        compressed.extend_from_slice(&(!length).to_le_bytes());
        compressed.extend_from_slice(&scanline);
        compressed.extend_from_slice(&adler32(&scanline).to_be_bytes());
        append_png_chunk(&mut bytes, b"IDAT", &compressed);
        append_png_chunk(&mut bytes, b"IEND", &[]);
        bytes
    }

    fn append_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], payload: &[u8]) {
        output.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("synthetic PNG chunk length")
                .to_be_bytes(),
        );
        output.extend_from_slice(chunk_type);
        output.extend_from_slice(payload);
        let mut crc = PngCrc::new();
        crc.update(chunk_type);
        crc.update(payload);
        output.extend_from_slice(&crc.finish().to_be_bytes());
    }

    fn adler32(bytes: &[u8]) -> u32 {
        const MODULUS: u32 = 65_521;
        let mut first = 1_u32;
        let mut second = 0_u32;
        for byte in bytes {
            first = (first + u32::from(*byte)) % MODULUS;
            second = (second + first) % MODULUS;
        }
        (second << 16) | first
    }

    fn synthetic_ogg(flags: u8) -> Vec<u8> {
        let payload = b"original-synthetic-ogg-packet";
        let mut page = vec![0_u8; 28];
        page[..4].copy_from_slice(b"OggS");
        page[5] = flags;
        page[14..18].copy_from_slice(&0x4531_0001_u32.to_le_bytes());
        page[26] = 1;
        page[27] = u8::try_from(payload.len()).expect("small Ogg packet");
        page.extend_from_slice(payload);
        let mut checksum = OggChecksum::new();
        checksum.update(&page);
        page[22..26].copy_from_slice(&checksum.finish().to_le_bytes());
        page
    }

    fn synthetic_wave(channels: u16, bits_per_sample: u16, data_length: Option<usize>) -> Vec<u8> {
        let bytes_per_sample = bits_per_sample / 8;
        let block_align = channels
            .checked_mul(bytes_per_sample)
            .expect("small synthetic WAVE block alignment");
        let sample_rate = 8_000_u32;
        let byte_rate = sample_rate
            .checked_mul(u32::from(block_align))
            .expect("small synthetic WAVE byte rate");
        let mut wave = b"RIFF\0\0\0\0WAVE".to_vec();
        wave.extend_from_slice(b"fmt ");
        wave.extend_from_slice(&16_u32.to_le_bytes());
        wave.extend_from_slice(&1_u16.to_le_bytes());
        wave.extend_from_slice(&channels.to_le_bytes());
        wave.extend_from_slice(&sample_rate.to_le_bytes());
        wave.extend_from_slice(&byte_rate.to_le_bytes());
        wave.extend_from_slice(&block_align.to_le_bytes());
        wave.extend_from_slice(&bits_per_sample.to_le_bytes());
        if let Some(data_length) = data_length {
            wave.extend_from_slice(b"data");
            wave.extend_from_slice(
                &u32::try_from(data_length)
                    .expect("small synthetic WAVE data length")
                    .to_le_bytes(),
            );
            wave.extend((0..data_length).map(|index| index as u8));
            if data_length % 2 != 0 {
                wave.push(0);
            }
        }
        rewrite_riff_length(&mut wave);
        wave
    }

    fn rewrite_riff_length(wave: &mut [u8]) {
        let riff_size = u32::try_from(wave.len() - 8).expect("small WAVE fixture");
        wave[4..8].copy_from_slice(&riff_size.to_le_bytes());
    }
}
