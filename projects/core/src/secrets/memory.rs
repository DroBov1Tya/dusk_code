//! A store in memory, for tests.

use super::{SecretStore, check_name};
use anyhow::Result;

/// A store in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(parking_lot::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        check_name(name)?;
        Ok(self.0.lock().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        check_name(name)?;
        self.0.lock().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.0.lock().remove(name);
        Ok(())
    }
}
