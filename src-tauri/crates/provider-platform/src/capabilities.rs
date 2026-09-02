use deltamod_product_contracts::{
    ProviderAccountState, ProviderAuthentication, ProviderCapability, ProviderDescriptor,
    ProviderDescriptorPayload, ProviderId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Providers normalized by this crate. Capabilities remain conservative until a documented
/// public interface supports them; external browser handoffs are not native search/download.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownProvider {
    GameBanana,
    GameJolt,
    Itch,
    ModDb,
    Nexus,
    Local,
}

impl KnownProvider {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GameBanana => "gamebanana",
            Self::GameJolt => "gamejolt",
            Self::Itch => "itch",
            Self::ModDb => "moddb",
            Self::Nexus => "nexus",
            Self::Local => "local",
        }
    }

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::GameBanana => "GameBanana",
            Self::GameJolt => "Game Jolt",
            Self::Itch => "itch.io",
            Self::ModDb => "ModDB",
            Self::Nexus => "Nexus Mods",
            Self::Local => "Local archive",
        }
    }

    #[must_use]
    pub fn from_id(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("gamebanana") {
            Some(Self::GameBanana)
        } else if value.eq_ignore_ascii_case("gamejolt") {
            Some(Self::GameJolt)
        } else if value.eq_ignore_ascii_case("itch") || value.eq_ignore_ascii_case("itch.io") {
            Some(Self::Itch)
        } else if value.eq_ignore_ascii_case("moddb") {
            Some(Self::ModDb)
        } else if value.eq_ignore_ascii_case("nexus") {
            Some(Self::Nexus)
        } else if value.eq_ignore_ascii_case("local") {
            Some(Self::Local)
        } else {
            None
        }
    }

    #[must_use]
    pub fn provider_id(self) -> ProviderId {
        ProviderId::parse(self.as_str()).expect("known provider IDs are contract-valid")
    }

    /// The static frozen descriptor. Conditional support is refined by
    /// [`KnownProvider::capability_report`].
    #[must_use]
    pub fn descriptor(self) -> ProviderDescriptor {
        let capabilities = match self {
            Self::GameBanana => BTreeSet::from([
                ProviderCapability::Search,
                ProviderCapability::Details,
                ProviderCapability::Versions,
                ProviderCapability::DirectDownload,
                ProviderCapability::ExternalDownload,
                ProviderCapability::Authentication,
                ProviderCapability::ModDiscovery,
            ]),
            Self::GameJolt => BTreeSet::from([
                ProviderCapability::ExternalDownload,
                ProviderCapability::ModDiscovery,
                ProviderCapability::GameAcquisition,
            ]),
            Self::Itch => BTreeSet::from([
                ProviderCapability::Versions,
                ProviderCapability::ExternalDownload,
                ProviderCapability::ModDiscovery,
                ProviderCapability::GameAcquisition,
            ]),
            Self::Nexus => BTreeSet::from([
                ProviderCapability::Search,
                ProviderCapability::Details,
                ProviderCapability::Versions,
                ProviderCapability::DirectDownload,
                ProviderCapability::ExternalDownload,
                ProviderCapability::Authentication,
                ProviderCapability::RateLimit,
                ProviderCapability::ModDiscovery,
            ]),
            Self::ModDb => BTreeSet::from([
                ProviderCapability::ExternalDownload,
                ProviderCapability::ModDiscovery,
            ]),
            Self::Local => BTreeSet::from([
                ProviderCapability::Details,
                ProviderCapability::ProviderInstall,
            ]),
        };
        let authentication = match self {
            Self::GameBanana | Self::Nexus => ProviderAuthentication::Optional,
            Self::GameJolt | Self::Itch | Self::ModDb | Self::Local => ProviderAuthentication::None,
        };
        ProviderDescriptor::new(ProviderDescriptorPayload {
            provider_id: self.provider_id(),
            display_name: self.display_name().to_owned(),
            capabilities,
            authentication,
        })
        .expect("known provider descriptors are contract-valid")
    }

    /// Reports operation-level support without treating an external handoff or local import as
    /// a direct provider download.
    #[must_use]
    pub const fn capability_report(
        self,
        account_state: ProviderAccountState,
    ) -> ProviderCapabilityReport {
        match self {
            Self::GameBanana => ProviderCapabilityReport {
                metadata: CapabilityAvailability::Available,
                search: CapabilityAvailability::Available,
                details: CapabilityAvailability::Available,
                versions: CapabilityAvailability::Available,
                direct_download: CapabilityAvailability::Available,
                external_download: CapabilityAvailability::Available,
                local_import: CapabilityAvailability::Unavailable,
            },
            Self::GameJolt => ProviderCapabilityReport {
                metadata: CapabilityAvailability::Available,
                search: CapabilityAvailability::Unavailable,
                details: CapabilityAvailability::Unavailable,
                versions: CapabilityAvailability::Unavailable,
                direct_download: CapabilityAvailability::Unavailable,
                external_download: CapabilityAvailability::Available,
                local_import: CapabilityAvailability::Unavailable,
            },
            Self::Itch => ProviderCapabilityReport {
                metadata: CapabilityAvailability::Available,
                search: CapabilityAvailability::Unavailable,
                details: CapabilityAvailability::Unavailable,
                versions: CapabilityAvailability::Available,
                direct_download: CapabilityAvailability::Unavailable,
                external_download: CapabilityAvailability::Available,
                local_import: CapabilityAvailability::Unavailable,
            },
            Self::Nexus => {
                let authenticated = matches!(account_state, ProviderAccountState::SignedIn);
                let authenticated_support = if authenticated {
                    CapabilityAvailability::Available
                } else {
                    CapabilityAvailability::AuthenticationRequired
                };
                ProviderCapabilityReport {
                    metadata: CapabilityAvailability::Available,
                    search: CapabilityAvailability::Available,
                    details: CapabilityAvailability::Available,
                    versions: authenticated_support,
                    direct_download: authenticated_support,
                    external_download: CapabilityAvailability::Available,
                    local_import: CapabilityAvailability::Unavailable,
                }
            }
            Self::ModDb => ProviderCapabilityReport {
                metadata: CapabilityAvailability::Available,
                search: CapabilityAvailability::Unavailable,
                details: CapabilityAvailability::Unavailable,
                versions: CapabilityAvailability::Unavailable,
                direct_download: CapabilityAvailability::Unavailable,
                external_download: CapabilityAvailability::Available,
                local_import: CapabilityAvailability::Unavailable,
            },
            Self::Local => ProviderCapabilityReport {
                metadata: CapabilityAvailability::Available,
                search: CapabilityAvailability::Unavailable,
                details: CapabilityAvailability::Available,
                versions: CapabilityAvailability::Unavailable,
                direct_download: CapabilityAvailability::Unavailable,
                external_download: CapabilityAvailability::Unavailable,
                local_import: CapabilityAvailability::Available,
            },
        }
    }

    #[must_use]
    pub const fn default_account_state(self) -> ProviderAccountState {
        match self {
            Self::GameBanana | Self::Nexus => ProviderAccountState::SignedOut,
            Self::GameJolt | Self::Itch | Self::ModDb | Self::Local => {
                ProviderAccountState::NotRequired
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    Available,
    AuthenticationRequired,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityReport {
    pub metadata: CapabilityAvailability,
    pub search: CapabilityAvailability,
    pub details: CapabilityAvailability,
    pub versions: CapabilityAvailability,
    pub direct_download: CapabilityAvailability,
    pub external_download: CapabilityAvailability,
    pub local_import: CapabilityAvailability,
}

#[must_use]
pub fn provider_descriptors() -> Vec<ProviderDescriptor> {
    [
        KnownProvider::GameBanana,
        KnownProvider::GameJolt,
        KnownProvider::Itch,
        KnownProvider::ModDb,
        KnownProvider::Nexus,
        KnownProvider::Local,
    ]
    .into_iter()
    .map(KnownProvider::descriptor)
    .collect()
}
