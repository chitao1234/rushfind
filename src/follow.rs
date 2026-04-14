#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowMode {
    Physical,
    CommandLineOnly,
    Logical,
}

impl FollowMode {
    pub fn follows_during_traversal(self, is_command_line_root: bool) -> bool {
        match self {
            Self::Physical => false,
            Self::CommandLineOnly => is_command_line_root,
            Self::Logical => true,
        }
    }
}
