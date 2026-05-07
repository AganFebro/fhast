#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveHeaderRetention {
    Never,
    UntilComplete,
    Encrypted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyConfig {
    pub sensitive_header_retention: SensitiveHeaderRetention,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            sensitive_header_retention: SensitiveHeaderRetention::UntilComplete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FhastConfig {
    pub privacy: PrivacyConfig,
}
