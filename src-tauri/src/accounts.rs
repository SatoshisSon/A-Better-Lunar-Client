//! Gespeicherte Accounts (mehrere, mit aktivem Account).

use serde::{Deserialize, Serialize};

use crate::auth::{self, McSession};
use crate::error::{Error, Result};
use crate::paths::{read_json_or_default, write_json, Paths};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub kind: AccountKind,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub refresh_token: String,
}

/// Account ohne Tokens – nur das geht an die Oberfläche.
#[derive(Debug, Clone, Serialize)]
pub struct AccountInfo {
    pub uuid: String,
    pub name: String,
    pub kind: AccountKind,
    pub active: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AccountStore {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

fn file(paths: &Paths) -> std::path::PathBuf {
    paths.file("accounts.json")
}

impl AccountStore {
    pub fn load(paths: &Paths) -> Self {
        read_json_or_default(&file(paths))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_json(&file(paths), self)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(file(paths), std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn infos(&self) -> Vec<AccountInfo> {
        self.accounts
            .iter()
            .map(|a| AccountInfo {
                uuid: a.uuid.clone(),
                name: a.name.clone(),
                kind: a.kind,
                active: self.active.as_deref() == Some(&a.uuid),
            })
            .collect()
    }

    /// Account hinzufügen oder ersetzen und aktiv setzen.
    pub fn upsert(&mut self, account: Account) {
        self.active = Some(account.uuid.clone());
        match self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }

    pub fn add_microsoft(&mut self, s: McSession) {
        self.upsert(Account {
            uuid: s.uuid,
            name: s.name,
            kind: AccountKind::Microsoft,
            access_token: s.access_token,
            expires_at: s.expires_at,
            refresh_token: s.refresh_token,
        });
    }

    pub fn add_offline(&mut self, name: &str) -> Result<()> {
        if !auth::valid_offline_name(name) {
            return Err(Error::msg("Name muss 3–16 Zeichen lang sein (Buchstaben, Zahlen, _)"));
        }
        self.upsert(Account {
            uuid: auth::offline_uuid(name),
            name: name.to_string(),
            kind: AccountKind::Offline,
            access_token: "0".into(),
            expires_at: u64::MAX,
            refresh_token: String::new(),
        });
        Ok(())
    }

    pub fn remove(&mut self, uuid: &str) {
        self.accounts.retain(|a| a.uuid != uuid);
        if self.active.as_deref() == Some(uuid) {
            self.active = self.accounts.first().map(|a| a.uuid.clone());
        }
    }

    pub fn set_active(&mut self, uuid: &str) -> Result<()> {
        if !self.accounts.iter().any(|a| a.uuid == uuid) {
            return Err(Error::msg("Account nicht gefunden"));
        }
        self.active = Some(uuid.to_string());
        Ok(())
    }

    pub fn active_account(&self) -> Option<&Account> {
        let uuid = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.uuid == uuid)
    }
}
