//! The providers' directories on the service's volume (`<root>/<program>`, what `provider_env` points each CLI at): made when a login starts,
//! looked in for the file the CLI keeps its login in, and emptied of that file alone on a logout. The service never opens the file: the CLI
//! wrote it and reads it, and what it holds does not pass through the service. Which provider is chosen is a file beside the directories.
use rca_cli_driver::domain::{credential_files, provider_env, Driver};
use rca_group_accounts::ProviderDirs;
use std::path::{Path, PathBuf};

pub struct ProviderFiles {
    root: PathBuf,
}

const SELECTED: &str = "selected";

impl ProviderFiles {
    pub fn at(root: impl Into<PathBuf>) -> ProviderFiles {
        ProviderFiles { root: root.into() }
    }

    fn own(&self, driver: Driver) -> PathBuf {
        self.root.join(driver.program())
    }
}

fn private(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

impl ProviderDirs for ProviderFiles {
    fn ensure(&self, driver: Driver) -> Result<(), String> {
        let root = self.root.to_string_lossy();
        private(&self.root)?;
        private(&self.own(driver))?;
        for (_, dir) in provider_env(driver, &root, &[]) {
            private(Path::new(&dir))?;
        }
        Ok(())
    }

    fn signed_in(&self, driver: Driver) -> bool {
        credential_files(driver).iter().any(|file| std::fs::metadata(self.own(driver).join(file)).is_ok_and(|m| m.is_file() && m.len() > 0))
    }

    fn clear(&self, driver: Driver) -> Result<(), String> {
        for file in credential_files(driver) {
            let path = self.own(driver).join(file);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        Ok(())
    }

    fn selected(&self) -> Option<Driver> {
        Driver::parse(std::fs::read_to_string(self.root.join(SELECTED)).ok()?.trim())
    }

    fn select(&self, driver: Driver) -> Result<(), String> {
        private(&self.root)?;
        std::fs::write(self.root.join(SELECTED), driver.id()).map_err(|e| format!("{}: {e}", self.root.join(SELECTED).display()))
    }

    fn deselect(&self) -> Result<(), String> {
        match std::fs::remove_file(self.root.join(SELECTED)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", self.root.join(SELECTED).display())),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_cli_driver::domain::DRIVERS;

    #[test]
    fn given_a_root_when_each_providers_directory_is_ensured_then_the_directories_its_cli_is_told_of_exist_privately_and_none_is_a_login() {
        // Given an empty root
        let root = tempfile::tempdir().unwrap();
        let dirs = ProviderFiles::at(root.path());
        for driver in DRIVERS {
            // When the provider's directory is ensured
            dirs.ensure(driver).unwrap();
            // Then every directory its environment names exists, and no login is there yet
            for (_, dir) in provider_env(driver, &root.path().to_string_lossy(), &[]) {
                assert!(Path::new(&dir).is_dir(), "{} lacks {dir}", driver.id());
            }
            assert!(!dirs.signed_in(driver));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(root.path().join("codex")).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn given_a_login_file_when_the_provider_is_asked_and_cleared_then_only_it_counts_and_only_it_goes_and_an_empty_file_is_no_login() {
        // Given each provider with its credential file and another file beside it, and one provider with an empty file
        let root = tempfile::tempdir().unwrap();
        let dirs = ProviderFiles::at(root.path());
        for driver in DRIVERS {
            dirs.ensure(driver).unwrap();
            let own = root.path().join(driver.program());
            std::fs::write(own.join("settings.json"), "{}").unwrap();
            for file in credential_files(driver) {
                std::fs::create_dir_all(own.join(file).parent().unwrap()).unwrap();
                std::fs::write(own.join(file), "x").unwrap();
            }
        }
        std::fs::write(root.path().join("codex").join(credential_files(Driver::Codex)[0]), "").unwrap();
        // When the providers are asked
        // Then a non-empty credential file is a login and an empty one is not
        assert_eq!(DRIVERS.map(|d| dirs.signed_in(d)), DRIVERS.map(|d| d != Driver::Codex));
        // And clearing one provider removes its credential file only, and clearing twice is no error
        dirs.clear(Driver::ClaudeAgent).unwrap();
        dirs.clear(Driver::ClaudeAgent).unwrap();
        assert!(!dirs.signed_in(Driver::ClaudeAgent) && dirs.signed_in(Driver::Opencode));
        assert!(root.path().join("claude/settings.json").is_file());
    }

    #[test]
    fn given_a_choice_when_selected_read_and_cleared_then_it_survives_a_new_adapter_over_the_same_root_and_an_unknown_name_is_none() {
        // Given a provider selected through one adapter
        let root = tempfile::tempdir().unwrap();
        ProviderFiles::at(root.path()).select(Driver::Codex).unwrap();
        // When another adapter over the same root reads it, and the choice is cleared (twice)
        let again = ProviderFiles::at(root.path());
        assert_eq!(again.selected(), Some(Driver::Codex));
        again.deselect().unwrap();
        again.deselect().unwrap();
        // Then nothing is selected, and a file naming no provider selects none
        assert_eq!(again.selected(), None);
        std::fs::write(root.path().join("selected"), "gemini").unwrap();
        assert_eq!(again.selected(), None);
    }
}
