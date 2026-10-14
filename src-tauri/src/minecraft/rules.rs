//! Auswertung der `rules` aus Versions-JSONs (Betriebssystem, Architektur, Features).

use super::manifest::{Library, Rule};

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

#[derive(Debug, Clone, Default)]
pub struct Features {
    pub has_custom_resolution: bool,
}

impl Features {
    fn get(&self, name: &str) -> bool {
        match name {
            "has_custom_resolution" => self.has_custom_resolution,
            _ => false,
        }
    }
}

fn rule_matches(rule: &Rule, features: &Features) -> bool {
    if let Some(os) = &rule.os {
        if os.name.as_deref().is_some_and(|n| n != os_name()) {
            return false;
        }
        if let Some(arch) = os.arch.as_deref() {
            let is_x86 = std::env::consts::ARCH == "x86";
            if (arch == "x86") != is_x86 {
                return false;
            }
        }
        // Versionsregeln betreffen nur uralte Betriebssysteme (z. B. OS X 10.5).
        if os.version.is_some() {
            return false;
        }
    }
    if let Some(f) = &rule.features {
        if f.iter().any(|(name, want)| features.get(name) != *want) {
            return false;
        }
    }
    true
}

pub fn allowed(rules: Option<&[Rule]>, features: &Features) -> bool {
    let Some(rules) = rules else { return true };
    if rules.is_empty() {
        return true;
    }
    let mut allow = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allow = rule.action == "allow";
        }
    }
    allow
}

pub fn library_allowed(lib: &Library) -> bool {
    allowed(lib.rules.as_deref(), &Features::default())
}

/// Klassifizierer für alte Natives-Bibliotheken (z. B. `natives-windows`).
pub fn native_classifier(lib: &Library) -> Option<String> {
    let natives = lib.natives.as_ref()?;
    let classifier = natives.get(os_name())?;
    let bits = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    Some(classifier.replace("${arch}", bits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::manifest::OsRule;

    fn rule(action: &str, os: Option<&str>) -> Rule {
        Rule {
            action: action.into(),
            os: os.map(|n| OsRule { name: Some(n.into()), arch: None, version: None }),
            features: None,
        }
    }

    #[test]
    fn allow_then_disallow_current_os() {
        let rules = vec![rule("allow", None), rule("disallow", Some(os_name()))];
        assert!(!allowed(Some(&rules), &Features::default()));
    }

    #[test]
    fn only_other_os() {
        let other = if os_name() == "windows" { "osx" } else { "windows" };
        let rules = vec![rule("allow", Some(other))];
        assert!(!allowed(Some(&rules), &Features::default()));
        assert!(allowed(None, &Features::default()));
    }
}
