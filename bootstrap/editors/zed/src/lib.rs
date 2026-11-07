use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct Jazyk;

impl zed::Extension for Jazyk {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let settings = LspSettings::for_worktree(id.as_ref(), worktree)?;
        let binary = settings.binary.as_ref();
        let command = binary
            .and_then(|b| b.path.clone())
            .or_else(|| worktree.which("jazyk"))
            .ok_or("Jazyk executable not found. Run `jazyk zed` from the project terminal.")?;
        let mut env = worktree.shell_env();
        if let Some(extra) = binary.and_then(|b| b.env.as_ref()) {
            env.retain(|(key, _)| !extra.contains_key(key));
            env.extend(
                extra
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
        Ok(zed::Command {
            command,
            args: binary
                .and_then(|b| b.arguments.clone())
                .unwrap_or_else(|| vec!["lsp".into()]),
            env,
        })
    }
}

zed::register_extension!(Jazyk);
