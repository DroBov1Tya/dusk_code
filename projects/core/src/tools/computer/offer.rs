//! How the computer tools are offered: one macro for the parts every one of them shares.

/// What the tools share: how they are offered.
macro_rules! computer_tool {
    ($name:literal) => {
        fn name(&self) -> &str {
            $name
        }

        fn kind(&self) -> ToolKind {
            ToolKind::Computer
        }

        fn needs(&self) -> Needs {
            Needs::Gui
        }

        /// One desktop and shared keyboard: the chat drives them, never its agents.
        fn root_only(&self) -> bool {
            true
        }
    };
}
