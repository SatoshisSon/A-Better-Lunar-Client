//! Optionaler Discord-Status („Spielt Minecraft 1.21.1 mit Fabric“).

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

#[derive(Default)]
pub struct Discord {
    client: Option<DiscordIpcClient>,
}

impl Discord {
    pub fn set_playing(&mut self, app_id: &str, details: &str, state: &str, started_at: i64) {
        if app_id.trim().is_empty() {
            return;
        }
        if self.client.is_none() {
            let mut client = DiscordIpcClient::new(app_id.trim());
            if client.connect().is_err() {
                return; // Discord läuft nicht – kein Problem.
            }
            self.client = Some(client);
        }
        if let Some(client) = &mut self.client {
            let act = activity::Activity::new()
                .details(details)
                .state(state)
                .timestamps(activity::Timestamps::new().start(started_at))
                .assets(
                    activity::Assets::new()
                        .large_image("logo")
                        .large_text("A Better Lunar Client"),
                );
            if client.set_activity(act).is_err() {
                self.client = None;
            }
        }
    }

    pub fn clear(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
    }
}
