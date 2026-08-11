#![forbid(unsafe_code)]
#![deny(missing_debug_implementations, missing_copy_implementations)]

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainError(pub String);
impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DomainError {}
type Result<T> = std::result::Result<T, DomainError>;

fn id<const N: usize>(value: &str, label: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > N
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || !value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
    {
        return Err(DomainError(format!("invalid {label}")));
    }
    Ok(value.to_owned())
}

macro_rules! id_type {
    ($name:ident, $label:literal, $max:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: &str) -> Result<Self> {
                Ok(Self(id::<$max>(value, $label)?))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl FromStr for $name {
            type Err = DomainError;
            fn from_str(s: &str) -> Result<Self> {
                Self::new(s)
            }
        }
    };
}
id_type!(ModFolderId, "mod folder ID", 64);
id_type!(ModUid, "mod UID", 128);
id_type!(VariantId, "variant ID", 64);
id_type!(ThemeId, "theme ID", 64);
id_type!(SponsorId, "sponsor ID", 64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModVariant {
    pub id: VariantId,
    pub label: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModRecord {
    pub uid: ModUid,
    pub folder: ModFolderId,
    pub name: String,
    pub variants: Vec<ModVariant>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModListState {
    pub mods: Vec<ModRecord>,
    pub enabled: Vec<ModUid>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModPlan {
    RemoveEnabled(ModUid),
    DeleteFolder(PathBuf),
}

pub fn normalize_enabled(enabled: &[ModUid]) -> Vec<ModUid> {
    let mut seen = HashSet::new();
    enabled
        .iter()
        .filter(|uid| seen.insert((*uid).clone()))
        .cloned()
        .collect()
}
pub fn plan_enabled_removal(enabled: &[ModUid], uid: &ModUid) -> Option<Vec<ModUid>> {
    let mut result = normalize_enabled(enabled);
    let before = result.len();
    result.retain(|item| item != uid);
    (before != result.len()).then_some(result)
}
pub fn validate_declared_variants(
    declared: &[ModVariant],
    selected: Option<&VariantId>,
) -> Result<()> {
    let mut ids = HashSet::new();
    for variant in declared {
        if !ids.insert(&variant.id) {
            return Err(DomainError("duplicate variant ID".into()));
        }
    }
    if let Some(selected) = selected {
        if !ids.contains(selected) {
            return Err(DomainError("selected variant is not declared".into()));
        }
    }
    Ok(())
}
pub fn plan_mod_deletion(packet_root: &Path, folder: &ModFolderId) -> Result<ModPlan> {
    let path = packet_root.join(folder.as_str());
    if !path.starts_with(packet_root) || path == packet_root {
        return Err(DomainError("mod deletion escaped packet root".into()));
    }
    Ok(ModPlan::DeleteFolder(path))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThemeRecord {
    pub id: ThemeId,
    pub name: String,
    pub built_in: bool,
    pub icon: Option<AssetRef>,
    pub music: Option<AssetRef>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThemeImport {
    pub id: ThemeId,
    pub name: String,
    pub files: Vec<AssetInput>,
}
pub fn validate_theme_import(import: &ThemeImport) -> Result<()> {
    if import.name.trim().is_empty() || import.name.len() > 128 {
        return Err(DomainError("invalid theme name".into()));
    }
    let mut paths = HashSet::new();
    for file in &import.files {
        validate_asset_input(file)?;
        if !paths.insert(file.name.clone()) {
            return Err(DomainError("duplicate theme asset".into()));
        }
    }
    Ok(())
}
pub fn reject_builtin_mutation(theme: &ThemeRecord) -> Result<()> {
    if theme.built_in {
        Err(DomainError("built-in themes are immutable".into()))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetInput {
    pub name: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRef(String);
impl AssetRef {
    pub fn new(value: &str) -> Result<Self> {
        validate_relative_asset(value)?;
        Ok(Self(value.replace('\\', "/")))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
pub fn validate_relative_asset(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || value.starts_with("//")
        || value.contains('%')
    {
        return Err(DomainError("insecure asset reference".into()));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(DomainError("insecure asset reference".into()));
        }
    }
    Ok(())
}
pub fn validate_asset_request(kind: &str, reference: &AssetRef) -> Result<()> {
    if !matches!(kind, "app" | "theme" | "packet") {
        return Err(DomainError("unknown asset kind".into()));
    }
    validate_relative_asset(reference.as_str())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetType {
    Png,
    Jpeg,
    Webp,
    Gif,
    Mp3,
    Ogg,
    Wav,
    Mp4,
}
impl AssetType {
    fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
            Self::Wav => "wav",
            Self::Mp4 => "mp4",
        }
    }
    fn magic(&self, b: &[u8]) -> bool {
        match self {
            Self::Png => b.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => b.starts_with(&[0xff, 0xd8, 0xff]),
            Self::Webp => b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP",
            Self::Gif => b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a"),
            Self::Mp3 => {
                b.starts_with(b"ID3") || (b.len() >= 2 && b[0] == 0xff && b[1] & 0xe0 == 0xe0)
            }
            Self::Ogg => b.starts_with(b"OggS"),
            Self::Wav => b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WAVE",
            Self::Mp4 => b.len() >= 12 && &b[4..8] == b"ftyp",
        }
    }
}
pub fn validate_asset_input(input: &AssetInput) -> Result<AssetType> {
    validate_relative_asset(&input.name)?;
    let ext = Path::new(&input.name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| DomainError("asset has no extension".into()))?;
    let kind = [
        AssetType::Png,
        AssetType::Jpeg,
        AssetType::Webp,
        AssetType::Gif,
        AssetType::Mp3,
        AssetType::Ogg,
        AssetType::Wav,
        AssetType::Mp4,
    ]
    .into_iter()
    .find(|kind| ext == kind.extension())
    .ok_or_else(|| DomainError("unsupported asset extension".into()))?;
    if !kind.magic(&input.bytes) {
        return Err(DomainError(
            "asset signature does not match extension".into(),
        ));
    }
    Ok(kind)
}
pub fn generated_asset_basename(
    id: &ThemeId,
    source_name: &str,
    used: &HashSet<String>,
) -> Result<String> {
    validate_relative_asset(source_name)?;
    let ext = Path::new(source_name)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| DomainError("asset has no extension".into()))?
        .to_ascii_lowercase();
    if ![
        "png", "jpg", "jpeg", "webp", "gif", "mp3", "ogg", "wav", "mp4",
    ]
    .contains(&ext.as_str())
    {
        return Err(DomainError("unsupported asset extension".into()));
    }
    let stem = format!("{}-asset", id.as_str());
    let mut candidate = format!("{stem}.{ext}");
    let mut n = 2;
    while used.contains(&candidate) {
        candidate = format!("{stem}-{n}.{ext}");
        n += 1;
    }
    Ok(candidate)
}
pub fn active_deletion_fallback(
    active: &ThemeId,
    deleting: &ThemeId,
    available: &[ThemeId],
    default: &ThemeId,
) -> Result<ThemeId> {
    if active != deleting {
        return Ok(active.clone());
    }
    if available.iter().any(|id| id == default && id != deleting) {
        return Ok(default.clone());
    }
    available
        .iter()
        .find(|id| *id != deleting)
        .cloned()
        .ok_or_else(|| DomainError("no theme fallback available".into()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsorManifest {
    pub id: SponsorId,
    pub display_name: String,
    pub root: Option<AssetRef>,
}
pub fn sponsor_root(manifest: &SponsorManifest) -> Option<&AssetRef> {
    manifest.root.as_ref()
}
pub fn validate_sponsor_manifest(manifest: &SponsorManifest) -> Result<()> {
    if manifest.display_name.trim().is_empty() {
        return Err(DomainError("invalid sponsor display name".into()));
    }
    if let Some(root) = &manifest.root {
        validate_asset_request("app", root)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Flag {
    Audio,
    Sfx,
    Controller,
    Setup,
}
impl Flag {
    fn token(&self) -> &'static str {
        match self {
            Self::Audio => "AUDIO",
            Self::Sfx => "SFX",
            Self::Controller => "CONTROLLER",
            Self::Setup => "SETUP",
        }
    }
}
pub fn parse_flags_exact(input: &str) -> Result<Vec<Flag>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for token in input.split_whitespace() {
        let flag = match token {
            "AUDIO" => Flag::Audio,
            "SFX" => Flag::Sfx,
            "CONTROLLER" => Flag::Controller,
            "SETUP" => Flag::Setup,
            _ => return Err(DomainError("unknown flag".into())),
        };
        if !seen.insert(flag) {
            return Err(DomainError("duplicate flag".into()));
        }
        out.push(flag);
    }
    Ok(out)
}
pub fn write_flags_exact(flags: &[Flag]) -> Result<String> {
    let mut seen = HashSet::new();
    for flag in flags {
        if !seen.insert(flag) {
            return Err(DomainError("duplicate flag".into()));
        }
    }
    Ok(flags.iter().map(Flag::token).collect::<Vec<_>>().join(" "))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariableType {
    Text,
    Integer,
    Boolean,
    Color,
    Asset,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedVariable {
    pub name: String,
    pub kind: VariableType,
    pub value: String,
}
pub fn validate_shared_variables(vars: &[SharedVariable]) -> Result<()> {
    let mut names = HashSet::new();
    for var in vars {
        if !id::<64>(&var.name, "variable name").is_ok() || !names.insert(&var.name) {
            return Err(DomainError("invalid or duplicate shared variable".into()));
        }
        if matches!(var.kind, VariableType::Asset) {
            AssetRef::new(&var.value)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentState {
    pub mods: ModListState,
    pub themes: Vec<ThemeRecord>,
    pub active_theme: ThemeId,
    pub sponsors: Vec<SponsorManifest>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomainResult<T> {
    pub value: T,
    pub warnings: Vec<String>,
}

pub trait FileMutationPlan {
    fn apply(&self) -> Result<()>;
}
pub trait DomainStore {
    fn current(&self) -> Result<CurrentState>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn uid(s: &str) -> ModUid {
        ModUid::new(s).unwrap()
    }
    fn tid(s: &str) -> ThemeId {
        ThemeId::new(s).unwrap()
    }
    #[test]
    fn ids_are_strict() {
        assert!(ModFolderId::new("../x").is_err());
        assert!(VariantId::new("x y").is_err());
        assert!(ThemeId::new("").is_err());
        assert!(ModUid::new("a/b").is_err());
    }
    #[test]
    fn enabled_is_stable_deduped_and_removable() {
        let a = uid("a");
        let b = uid("b");
        assert_eq!(
            normalize_enabled(&[a.clone(), b.clone(), a.clone()]),
            vec![a.clone(), b]
        );
        assert_eq!(
            plan_enabled_removal(&[a.clone(), a.clone()], &a),
            Some(vec![])
        );
    }
    #[test]
    fn variants_must_be_declared_once() {
        let a = VariantId::new("default").unwrap();
        assert!(validate_declared_variants(
            &[
                ModVariant {
                    id: a.clone(),
                    label: "A".into()
                },
                ModVariant {
                    id: a.clone(),
                    label: "B".into()
                }
            ],
            None
        )
        .is_err());
        assert!(validate_declared_variants(&[], Some(&a)).is_err());
    }
    #[test]
    fn deletion_is_root_constrained() {
        let root = Path::new("C:/packets");
        let plan = plan_mod_deletion(root, &ModFolderId::new("mod-1").unwrap()).unwrap();
        assert_eq!(plan, ModPlan::DeleteFolder(root.join("mod-1")));
    }
    #[test]
    fn signatures_are_checked() {
        let i = AssetInput {
            name: "x.PNG".into(),
            bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
        };
        assert_eq!(validate_asset_input(&i), Ok(AssetType::Png));
        assert!(validate_asset_input(&AssetInput {
            name: "x.png".into(),
            bytes: b"bad".to_vec()
        })
        .is_err());
    }
    #[test]
    fn all_media_signatures_are_supported() {
        let samples = [
            ("a.jpg", vec![0xff, 0xd8, 0xff], AssetType::Jpeg),
            ("a.webp", b"RIFF1234WEBP".to_vec(), AssetType::Webp),
            ("a.gif", b"GIF89a".to_vec(), AssetType::Gif),
            ("a.mp3", b"ID3".to_vec(), AssetType::Mp3),
            ("a.ogg", b"OggS".to_vec(), AssetType::Ogg),
            ("a.wav", b"RIFF1234WAVE".to_vec(), AssetType::Wav),
            ("a.mp4", b"0000ftyp1234".to_vec(), AssetType::Mp4),
        ];
        for (name, bytes, want) in samples {
            assert_eq!(
                validate_asset_input(&AssetInput {
                    name: name.into(),
                    bytes
                }),
                Ok(want)
            );
        }
    }
    #[test]
    fn secure_references_and_names() {
        assert!(AssetRef::new("../x.png").is_err());
        assert!(AssetRef::new("C:/x.png").is_err());
        let used = HashSet::from(["t-asset.png".into()]);
        assert_eq!(
            generated_asset_basename(&tid("t"), "x.PNG", &used).unwrap(),
            "t-asset-2.png"
        );
    }
    #[test]
    fn fallback_obeys_default_then_first() {
        let a = tid("a");
        let b = tid("b");
        assert_eq!(
            active_deletion_fallback(&a, &a, &[a.clone(), b.clone()], &b),
            Ok(b.clone())
        );
        assert_eq!(
            active_deletion_fallback(&a, &b, &[a.clone(), b.clone()], &a),
            Ok(a)
        );
    }
    #[test]
    fn flags_are_exact_allowlisted_unique() {
        assert_eq!(
            parse_flags_exact("AUDIO SETUP").unwrap(),
            vec![Flag::Audio, Flag::Setup]
        );
        assert!(parse_flags_exact("audio").is_err());
        assert!(parse_flags_exact("AUDIO AUDIO").is_err());
        assert_eq!(write_flags_exact(&[Flag::Sfx]), Ok("SFX".into()));
    }
    #[test]
    fn variables_are_typed_and_allowlisted() {
        assert!(validate_shared_variables(&[SharedVariable {
            name: "x".into(),
            kind: VariableType::Text,
            value: "ok".into()
        }])
        .is_ok());
        assert!(validate_shared_variables(&[SharedVariable {
            name: "x".into(),
            kind: VariableType::Asset,
            value: "../x".into()
        }])
        .is_err());
    }
    #[test]
    fn builtins_and_sponsors_are_safe() {
        let t = ThemeRecord {
            id: tid("base"),
            name: "Base".into(),
            built_in: true,
            icon: None,
            music: None,
        };
        assert!(reject_builtin_mutation(&t).is_err());
        let s = SponsorManifest {
            id: SponsorId::new("x").unwrap(),
            display_name: "X".into(),
            root: None,
        };
        assert!(validate_sponsor_manifest(&s).is_ok());
        assert!(sponsor_root(&s).is_none());
    }
}
