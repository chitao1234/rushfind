#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportLevel {
    Exact,
    Approximate(&'static str),
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformFeature {
    FsType,
    SameFileSystem,
    BirthTime,
    FileFlags,
    ReparseType,
    NamedOwnership,
    NumericOwnership,
    WindowsOwnershipPredicates,
    AccessPredicates,
    MessagesLocale,
    CaseInsensitiveGlob,
    ModeBits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputContract {
    #[cfg(any(unix, test))]
    Posix,
    #[cfg(any(windows, test))]
    WindowsNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlatformCapabilities {
    pub(crate) fstype: SupportLevel,
    pub(crate) same_file_system: SupportLevel,
    pub(crate) birth_time: SupportLevel,
    pub(crate) file_flags: SupportLevel,
    pub(crate) reparse_type: SupportLevel,
    pub(crate) named_ownership: SupportLevel,
    pub(crate) numeric_ownership: SupportLevel,
    pub(crate) windows_ownership_predicates: SupportLevel,
    pub(crate) access_predicates: SupportLevel,
    pub(crate) messages_locale: SupportLevel,
    pub(crate) case_insensitive_glob: SupportLevel,
    pub(crate) mode_bits: SupportLevel,
    pub(crate) output_contract: OutputContract,
}

impl PlatformCapabilities {
    pub(crate) const fn support(&self, feature: PlatformFeature) -> SupportLevel {
        match feature {
            PlatformFeature::FsType => self.fstype,
            PlatformFeature::SameFileSystem => self.same_file_system,
            PlatformFeature::BirthTime => self.birth_time,
            PlatformFeature::FileFlags => self.file_flags,
            PlatformFeature::ReparseType => self.reparse_type,
            PlatformFeature::NamedOwnership => self.named_ownership,
            PlatformFeature::NumericOwnership => self.numeric_ownership,
            PlatformFeature::WindowsOwnershipPredicates => self.windows_ownership_predicates,
            PlatformFeature::AccessPredicates => self.access_predicates,
            PlatformFeature::MessagesLocale => self.messages_locale,
            PlatformFeature::CaseInsensitiveGlob => self.case_insensitive_glob,
            PlatformFeature::ModeBits => self.mode_bits,
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_windows_native_output_contract(mut self) -> Self {
        self.output_contract = OutputContract::WindowsNative;
        self
    }

    pub(crate) const fn uses_windows_native_output_contract(&self) -> bool {
        #[cfg(any(windows, test))]
        {
            matches!(self.output_contract, OutputContract::WindowsNative)
        }

        #[cfg(not(any(windows, test)))]
        {
            false
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_tests() -> Self {
        Self {
            fstype: SupportLevel::Unsupported("unset"),
            same_file_system: SupportLevel::Unsupported("unset"),
            birth_time: SupportLevel::Unsupported("unset"),
            file_flags: SupportLevel::Unsupported("unset"),
            reparse_type: SupportLevel::Unsupported("unset"),
            named_ownership: SupportLevel::Unsupported("unset"),
            numeric_ownership: SupportLevel::Unsupported("unset"),
            windows_ownership_predicates: SupportLevel::Unsupported("unset"),
            access_predicates: SupportLevel::Unsupported("unset"),
            messages_locale: SupportLevel::Unsupported("unset"),
            case_insensitive_glob: SupportLevel::Unsupported("unset"),
            mode_bits: SupportLevel::Unsupported("unset"),
            output_contract: OutputContract::Posix,
        }
    }

    #[cfg(test)]
    pub(crate) const fn with(mut self, feature: PlatformFeature, support: SupportLevel) -> Self {
        match feature {
            PlatformFeature::FsType => self.fstype = support,
            PlatformFeature::SameFileSystem => self.same_file_system = support,
            PlatformFeature::BirthTime => self.birth_time = support,
            PlatformFeature::FileFlags => self.file_flags = support,
            PlatformFeature::ReparseType => self.reparse_type = support,
            PlatformFeature::NamedOwnership => self.named_ownership = support,
            PlatformFeature::NumericOwnership => self.numeric_ownership = support,
            PlatformFeature::WindowsOwnershipPredicates => {
                self.windows_ownership_predicates = support
            }
            PlatformFeature::AccessPredicates => self.access_predicates = support,
            PlatformFeature::MessagesLocale => self.messages_locale = support,
            PlatformFeature::CaseInsensitiveGlob => self.case_insensitive_glob = support,
            PlatformFeature::ModeBits => self.mode_bits = support,
        }
        self
    }
}
