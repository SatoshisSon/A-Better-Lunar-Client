//! Baut die Java-Kommandozeile zum Starten von Minecraft.

use std::collections::HashMap;
use std::path::Path;

use super::install::Installed;
use super::manifest::{ArgValue, Argument, VersionJson};
use super::rules::{self, Features};

/// Angepasste G1GC-Flags für weniger Ruckler (an Aikars Flags angelehnt).
pub const OPTIMIZED_JVM_FLAGS: &[&str] = &[
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:MaxGCPauseMillis=50",
    "-XX:+DisableExplicitGC",
    "-XX:G1NewSizePercent=30",
    "-XX:G1MaxNewSizePercent=40",
    "-XX:G1HeapRegionSize=8M",
    "-XX:G1ReservePercent=20",
    "-XX:G1HeapWastePercent=5",
    "-XX:G1MixedGCCountTarget=4",
    "-XX:InitiatingHeapOccupancyPercent=15",
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseIntervalPercent=5",
    "-XX:SurvivorRatio=32",
    "-XX:+PerfDisableSharedMem",
    "-XX:MaxTenuringThreshold=1",
];

pub struct Auth {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub xuid: String,
}

pub struct LaunchOptions<'a> {
    pub version: &'a VersionJson,
    pub installed: &'a Installed,
    pub game_dir: &'a Path,
    pub auth: &'a Auth,
    pub memory_mb: u32,
    pub optimized_flags: bool,
    pub extra_jvm_args: &'a str,
    pub resolution: Option<(u32, u32)>,
}

fn substitute(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 3 + end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn collect(args: &[Argument], features: &Features, out: &mut Vec<String>) {
    for arg in args {
        match arg {
            Argument::Plain(s) => out.push(s.clone()),
            Argument::Ruled { rules: r, value } => {
                if rules::allowed(Some(r), features) {
                    match value {
                        ArgValue::One(s) => out.push(s.clone()),
                        ArgValue::Many(v) => out.extend(v.iter().cloned()),
                    }
                }
            }
        }
    }
}

/// Einfache Aufteilung eigener JVM-Argumente (Anführungszeichen werden beachtet).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn build_args(o: &LaunchOptions) -> Vec<String> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath = o
        .installed
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep);

    let (width, height) = o.resolution.unwrap_or((854, 480));
    let features = Features { has_custom_resolution: o.resolution.is_some() };
    let path = |p: &Path| p.to_string_lossy().into_owned();

    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", o.auth.name.clone()),
        ("version_name", o.version.id.clone()),
        ("game_directory", path(o.game_dir)),
        ("assets_root", path(&o.installed.assets_root)),
        ("game_assets", path(&o.installed.game_assets)),
        ("assets_index_name", o.installed.assets_index_name.clone()),
        ("auth_uuid", o.auth.uuid.clone()),
        ("auth_access_token", o.auth.access_token.clone()),
        ("auth_session", format!("token:{}:{}", o.auth.access_token, o.auth.uuid)),
        ("auth_xuid", o.auth.xuid.clone()),
        ("clientid", String::new()),
        ("user_type", o.auth.user_type.clone()),
        ("user_properties", "{}".into()),
        ("version_type", o.version.kind.clone().unwrap_or_else(|| "release".into())),
        ("resolution_width", width.to_string()),
        ("resolution_height", height.to_string()),
        ("natives_directory", path(&o.installed.natives_dir)),
        ("launcher_name", "a-better-lunar-client".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
        ("classpath", classpath.clone()),
        ("classpath_separator", sep.into()),
        ("library_directory", path(&o.installed.libraries_dir)),
    ]);

    // --- JVM-Argumente ---
    let mut jvm = vec![format!("-Xms{}M", (o.memory_mb / 2).max(512)), format!("-Xmx{}M", o.memory_mb)];
    if o.optimized_flags {
        jvm.extend(OPTIMIZED_JVM_FLAGS.iter().map(|s| s.to_string()));
    }
    jvm.extend(split_args(o.extra_jvm_args));
    if let Some(arg) = &o.installed.logging_arg {
        jvm.push(arg.clone());
    }

    let mut version_jvm = Vec::new();
    if let Some(a) = &o.version.arguments {
        collect(&a.jvm, &features, &mut version_jvm);
    }
    if version_jvm.is_empty() {
        // Alte Versionen liefern keine JVM-Argumente mit.
        if cfg!(target_os = "macos") {
            version_jvm.push("-XstartOnFirstThread".into());
        }
        if cfg!(windows) {
            version_jvm.push(
                "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".into(),
            );
        }
        version_jvm.push("-Djava.library.path=${natives_directory}".into());
        version_jvm.push("-cp".into());
        version_jvm.push("${classpath}".into());
    }
    jvm.extend(version_jvm.iter().map(|a| substitute(a, &vars)));

    // --- Spiel-Argumente ---
    let mut game = Vec::new();
    if let Some(s) = &o.version.minecraft_arguments {
        game.extend(s.split_whitespace().map(String::from));
    }
    if let Some(a) = &o.version.arguments {
        collect(&a.game, &features, &mut game);
    }
    if o.resolution.is_some() && o.version.arguments.is_none() {
        game.extend(["--width", "${resolution_width}", "--height", "${resolution_height}"].map(String::from));
    }

    let mut args = jvm;
    args.push(o.version.main_class.clone());
    args.extend(game.iter().map(|a| substitute(a, &vars)));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_known_and_keeps_unknown() {
        let vars = HashMap::from([("a", "1".to_string())]);
        assert_eq!(substitute("x${a}y${b}z", &vars), "x1y${b}z");
        assert_eq!(substitute("${a", &vars), "${a");
    }

    #[test]
    fn splits_quoted_args() {
        assert_eq!(
            split_args(r#"-Dfoo="a b" -Xss2M  'c d'"#),
            vec!["-Dfoo=a b", "-Xss2M", "c d"]
        );
    }
}
