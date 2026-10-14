//! Anmeldung: Microsoft (Device-Code → Xbox Live → XSTS → Minecraft) und Offline.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};

const MS_DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// UUID wie bei Java `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")`.
pub fn offline_uuid(name: &str) -> String {
    let mut hash: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    hash[6] = (hash[6] & 0x0f) | 0x30;
    hash[8] = (hash[8] & 0x3f) | 0x80;
    hex::encode(hash)
}

pub fn valid_offline_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    #[serde(default)]
    pub message: String,
}

#[derive(Deserialize)]
struct MsToken {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct MsError {
    error: String,
    #[serde(default)]
    error_description: String,
}

/// Ergebnis einer erfolgreichen Anmeldung.
pub struct McSession {
    pub uuid: String,
    pub name: String,
    pub access_token: String,
    pub expires_at: u64,
    pub refresh_token: String,
}

fn require_client_id(client_id: &str) -> Result<()> {
    if client_id.trim().is_empty() {
        return Err(Error::msg(
            "Für den Microsoft-Login wird eine Azure-Client-ID benötigt. Trage sie unter Einstellungen ein (siehe README).",
        ));
    }
    Ok(())
}

pub async fn start_device_code(http: &reqwest::Client, client_id: &str) -> Result<DeviceCode> {
    require_client_id(client_id)?;
    let resp = http
        .post(MS_DEVICE_CODE)
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await?;
    if !resp.status().is_success() {
        let err: MsError = resp.json().await?;
        return Err(Error::msg(format!("Microsoft-Login fehlgeschlagen: {}", err.error_description)));
    }
    Ok(resp.json().await?)
}

/// Wartet, bis der Nutzer den Code im Browser bestätigt hat.
pub async fn poll_device_code(
    http: &reqwest::Client,
    client_id: &str,
    code: &DeviceCode,
    cancelled: impl Fn() -> bool,
) -> Result<McSession> {
    let deadline = now() + code.expires_in;
    let mut interval = code.interval.max(1);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if cancelled() {
            return Err(Error::msg("Anmeldung abgebrochen"));
        }
        if now() > deadline {
            return Err(Error::msg("Der Code ist abgelaufen. Bitte erneut versuchen."));
        }
        let resp = http
            .post(MS_TOKEN)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", code.device_code.as_str()),
            ])
            .send()
            .await?;
        if resp.status().is_success() {
            let token: MsToken = resp.json().await?;
            return minecraft_login(http, &token.access_token, token.refresh_token).await;
        }
        let err: MsError = resp.json().await?;
        match err.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            "authorization_declined" => return Err(Error::msg("Anmeldung wurde abgelehnt")),
            "expired_token" => return Err(Error::msg("Der Code ist abgelaufen")),
            _ => return Err(Error::msg(format!("Microsoft-Login fehlgeschlagen: {}", err.error_description))),
        }
    }
}

/// Erneuert die Sitzung mit dem gespeicherten Refresh-Token.
pub async fn refresh(http: &reqwest::Client, client_id: &str, refresh_token: &str) -> Result<McSession> {
    require_client_id(client_id)?;
    let resp = http
        .post(MS_TOKEN)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::msg("Die Anmeldung ist abgelaufen. Bitte melde den Account neu an."));
    }
    let token: MsToken = resp.json().await?;
    minecraft_login(http, &token.access_token, token.refresh_token).await
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: serde_json::Value,
}

#[derive(Deserialize)]
struct McLoginResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
}

async fn minecraft_login(http: &reqwest::Client, ms_token: &str, refresh_token: String) -> Result<McSession> {
    // Xbox Live
    let xbl: XboxResponse = http
        .post(XBL_AUTH)
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let uhs = xbl.display_claims["xui"][0]["uhs"]
        .as_str()
        .ok_or_else(|| Error::msg("Xbox-Antwort ohne Benutzer-Hash"))?
        .to_string();

    // XSTS
    let resp = http
        .post(XSTS_AUTH)
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await?;
    if resp.status().as_u16() == 401 {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let msg = match body["XErr"].as_u64() {
            Some(2148916233) => "Dieser Microsoft-Account hat kein Xbox-Profil. Erstelle eins auf xbox.com.",
            Some(2148916235) => "Xbox Live ist in deinem Land nicht verfügbar.",
            Some(2148916236) | Some(2148916237) => "Der Account muss auf xbox.com volljährig verifiziert werden.",
            Some(2148916238) => "Kinder-Account: Er muss von einem Erwachsenen zu einer Familie hinzugefügt werden.",
            _ => "Xbox-Anmeldung fehlgeschlagen.",
        };
        return Err(Error::msg(msg));
    }
    let xsts: XboxResponse = resp.error_for_status()?.json().await?;

    // Minecraft
    let mc: McLoginResponse = http
        .post(MC_LOGIN)
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token) }))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| {
            if e.status().map(|s| s.as_u16()) == Some(403) {
                Error::msg("Die Azure-App ist noch nicht für Minecraft freigeschaltet (siehe README).")
            } else {
                e.into()
            }
        })?
        .json()
        .await?;

    let resp = http.get(MC_PROFILE).bearer_auth(&mc.access_token).send().await?;
    if resp.status().as_u16() == 404 {
        return Err(Error::msg("Dieser Account besitzt Minecraft: Java Edition nicht."));
    }
    let profile: McProfile = resp.error_for_status()?.json().await?;

    Ok(McSession {
        uuid: profile.id,
        name: profile.name,
        access_token: mc.access_token,
        expires_at: now() + mc.expires_in,
        refresh_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java() {
        // Java: UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))
        assert_eq!(offline_uuid("Notch"), "b50ad385829d3141a2167e7d7539ba7f");
    }

    #[test]
    fn offline_names() {
        assert!(valid_offline_name("Steve_123"));
        assert!(!valid_offline_name("ab"));
        assert!(!valid_offline_name("bad name"));
    }
}
