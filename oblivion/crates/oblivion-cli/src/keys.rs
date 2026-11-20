//! API keys (SRS FR-103): Windows Credential Manager (or the macOS Keychain /
//! Linux Secret Service), with environment variables as an override.

use anyhow::{bail, Context, Result};
use oblivion_core::config::ProviderEntry;

const SERVICE: &str = "oblivion";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Env,
    Store,
}

fn from_store(provider: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, provider)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// Environment variable first (so a one-off override works), then the key store.
pub fn lookup(provider: &str, env_var: Option<&str>) -> Option<(String, KeySource)> {
    if let Some(var) = env_var {
        if let Ok(v) = std::env::var(var) {
            if !v.trim().is_empty() {
                return Some((v, KeySource::Env));
            }
        }
    }
    from_store(provider).map(|k| (k, KeySource::Store))
}

pub fn for_provider(p: &ProviderEntry) -> Option<String> {
    lookup(&p.name, p.api_key_env.as_deref()).map(|(k, _)| k)
}

pub fn save(provider: &str, key: &str) -> Result<()> {
    if provider.trim().is_empty() || provider.contains(char::is_whitespace) {
        bail!("invalid provider name: {provider}");
    }
    let key = key.trim();
    if key.is_empty() {
        bail!("no key entered; nothing saved");
    }
    if key.chars().any(char::is_whitespace) || key.len() > 200 {
        bail!(
            "that does not look like an API key ({} characters, contains spaces)",
            key.chars().count()
        );
    }
    keyring::Entry::new(SERVICE, provider)
        .and_then(|e| e.set_password(key))
        .context("could not save the key in the system key store")?;
    Ok(())
}

pub fn set(provider: &str) -> Result<()> {
    if provider.trim().is_empty() || provider.contains(char::is_whitespace) {
        bail!("give the provider name from oblivion.toml, e.g. `oblivion --set-key groq`");
    }
    let key = rpassword::prompt_password(format!(
        "Paste the API key for {provider} (input is hidden): "
    ))
    .context("could not read the key from the terminal")?;
    save(provider, &key)?;
    println!(
        "Saved the {provider} key ({} characters) in the system key store.",
        key.trim().chars().count()
    );
    if let Ok(v) = std::env::var(format!("{}_API_KEY", provider.to_ascii_uppercase())) {
        if !v.trim().is_empty() {
            println!(
                "Note: the environment variable {}_API_KEY is also set and takes priority. \
                 Remove it if it holds an old key.",
                provider.to_ascii_uppercase()
            );
        }
    }
    Ok(())
}

pub fn delete(provider: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, provider)
        .and_then(|e| e.delete_credential())
        .with_context(|| format!("no saved key for {provider}"))?;
    println!("Deleted the saved {provider} key.");
    Ok(())
}
