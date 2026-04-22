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
    AccessPredicates,
    MessagesLocale,
    CaseInsensitiveGlob,
    ModeBits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlatformCapabilities {
    fstype: SupportLevel,
    same_file_system: SupportLevel,
    birth_time: SupportLevel,
    file_flags: SupportLevel,
    reparse_type: SupportLevel,
    named_ownership: SupportLevel,
    numeric_ownership: SupportLevel,
    access_predicates: SupportLevel,
    messages_locale: SupportLevel,
    case_insensitive_glob: SupportLevel,
    mode_bits: SupportLevel,
}

impl PlatformCapabilities {
    pub(crate) const fn new(
        fstype: SupportLevel,
        same_file_system: SupportLevel,
        birth_time: SupportLevel,
        file_flags: SupportLevel,
        reparse_type: SupportLevel,
        named_ownership: SupportLevel,
        numeric_ownership: SupportLevel,
        access_predicates: SupportLevel,
        messages_locale: SupportLevel,
        case_insensitive_glob: SupportLevel,
        mode_bits: SupportLevel,
    ) -> Self {
        Self {
            fstype,
            same_file_system,
            birth_time,
            file_flags,
            reparse_type,
            named_ownership,
            numeric_ownership,
            access_predicates,
            messages_locale,
            case_insensitive_glob,
            mode_bits,
        }
    }

    pub(crate) const fn support(self, feature: PlatformFeature) -> SupportLevel {
        match feature {
            PlatformFeature::FsType => self.fstype,
            PlatformFeature::SameFileSystem => self.same_file_system,
            PlatformFeature::BirthTime => self.birth_time,
            PlatformFeature::FileFlags => self.file_flags,
            PlatformFeature::ReparseType => self.reparse_type,
            PlatformFeature::NamedOwnership => self.named_ownership,
            PlatformFeature::NumericOwnership => self.numeric_ownership,
            PlatformFeature::AccessPredicates => self.access_predicates,
            PlatformFeature::MessagesLocale => self.messages_locale,
            PlatformFeature::CaseInsensitiveGlob => self.case_insensitive_glob,
            PlatformFeature::ModeBits => self.mode_bits,
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_tests() -> Self {
        Self::new(
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
            SupportLevel::Unsupported("unset"),
        )
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
            PlatformFeature::AccessPredicates => self.access_predicates = support,
            PlatformFeature::MessagesLocale => self.messages_locale = support,
            PlatformFeature::CaseInsensitiveGlob => self.case_insensitive_glob = support,
            PlatformFeature::ModeBits => self.mode_bits = support,
        }
        self
    }
}
