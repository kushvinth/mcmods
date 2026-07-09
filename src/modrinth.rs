use anyhow::{Context, Result};
use serde::Deserialize;

pub const API_BASE: &str = "https://api.modrinth.com/v3";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Project {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub version_type: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionFile {
    pub hashes: FileHashes,
    pub url: String,
    pub filename: String,
    pub primary: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct FileHashes {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Dependency {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub author: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub downloads: i64,
    #[serde(default)]
    pub follows: i64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total_hits: i64,
    pub offset: i64,
    pub limit: i64,
}

pub async fn search_projects(query: &str) -> Result<Vec<SearchHit>> {
    let url = format!(
        "{API_BASE}/search?query={}&limit=20&index=relevance",
        urlencode(query)
    );
    let resp = client()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to search Modrinth for '{query}'"))?;
    let body: SearchResponse = resp.json().await?;
    Ok(body.hits)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("mcmod/0.1.2 (github_user/mcmod)")
        .build()
        .expect("failed to build HTTP client")
}

pub async fn get_project(slug_or_id: &str) -> Result<Project> {
    let url = format!("{API_BASE}/project/{slug_or_id}");
    let resp = client()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to get project '{slug_or_id}' from Modrinth"))?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "Modrinth API returned {} for project '{}'",
            resp.status(),
            slug_or_id
        );
    }
    let project: Project = resp.json().await?;
    Ok(project)
}

pub async fn get_all_versions(project_id: &str) -> Result<Vec<Version>> {
    let url = format!("{API_BASE}/project/{project_id}/version");
    let resp = client()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to get all versions for '{project_id}'"))?;
    let versions: Vec<Version> = resp.json().await?;
    Ok(versions)
}

pub async fn get_versions(
    project_id: &str,
    game_version: &str,
    loader: &str,
) -> Result<Vec<Version>> {
    let loaders_q = format!("[\"{}\"]", loader);
    let game_versions_q = format!("[\"{}\"]", game_version);
    let url = format!(
        "{API_BASE}/project/{project_id}/version?loaders={}&game_versions={}",
        urlencode(&loaders_q),
        urlencode(&game_versions_q),
    );
    let resp = client()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to get versions for project '{project_id}'"))?;
    let versions: Vec<Version> = resp.json().await?;
    Ok(versions)
}

pub fn guess_slug_from_filename(filename: &str) -> Option<String> {
    let lower = filename.to_lowercase();
    let stem = lower.strip_suffix(".jar")?;

    let known_slugs: &[(&str, &str)] = &[
        ("sodium-fabric", "sodium"),
        ("sodium-extra-fabric", "sodium-extra"),
        ("iris-fabric", "iris"),
        ("fabric-api", "fabric-api"),
        ("lithium-fabric", "lithium"),
        ("modmenu", "modmenu"),
        ("appleskin-fabric", "appleskin"),
        ("xaero's minimap", "xaeros-minimap"),
        ("xaero's world map", "xaeros-world-map"),
        ("meteor-client", "meteor-client"),
        ("freecam-fabric", "freecam"),
        ("combat-hitbox", "combat-hitboxes"),
        ("gamma-utils", "gamma-utils"),
        ("shulkerboxtooltip-fabric", "shulkerboxtooltip"),
        ("entityculling-fabric", "entityculling"),
        ("ferritecore", "ferrite-core"),
        ("immediatelyfast-fabric", "immediatelyfast"),
        ("proxmity-voice-chat", "proximity-voice-chat"),
        ("ukulib", "ukulib"),
        ("ukus-armor-hud", "ukus-armor-hud"),
        ("wi-zoom", "wi-zoom"),
        ("vt-downloader", "vt-downloader"),
        ("locator-heads", "locator-heads"),
        ("fixmyping", "fixmyping"),
        ("smoothscroll", "smoothscroll"),
        ("smoothswapping", "smoothswapping"),
        ("worldedit", "worldedit"),
        ("litematica", "litematica"),
        ("malilib", "malilib"),
        ("controlling", "controlling"),
        ("searchables", "searchables"),
        ("dynamic fullbright", "dynamic-fullbright"),
        ("crosshair addons", "crosshair-addons"),
        ("shield fixes", "shield-fixes"),
        ("walksy lib", "walksy-lib"),
        ("bactromod", "bactromod"),
        ("balm", "balm"),
        ("cloth config", "cloth-config"),
        ("herobot", "herobot"),
        ("natural motion blur", "natural-motion-blur"),
        (
            "phase's discord rich presence",
            "phases-discord-rich-presence",
        ),
        ("status effect timer", "status-effect-timer"),
        ("totemcounter", "totemcounter"),
        ("totem tweaks", "totem-tweaks"),
        ("yacl", "yet-another-config-lib"),
        ("drippy loading screen", "drippy-loading-screen"),
        ("fancymenu", "fancymenu"),
        ("konkrete", "konkrete"),
        ("melody", "melody"),
        ("voicechat", "simple-voice-chat"),
        ("xaerominimap", "xaeros-minimap"),
    ];

    for (pattern, slug) in known_slugs {
        if stem.contains(pattern) {
            return Some(slug.to_string());
        }
    }

    None
}

fn urlencode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_project_sodium() {
        let project = get_project("sodium").await.unwrap();
        assert_eq!(project.slug, "sodium");
        assert!(!project.id.is_empty());
        println!("sodium: id={}, title={}", project.id, project.title);
    }

    #[tokio::test]
    async fn test_get_versions_sodium() {
        let project = get_project("sodium").await.unwrap();
        let versions = get_versions(&project.id, "1.21.11", "fabric")
            .await
            .unwrap();
        assert!(!versions.is_empty(), "should have at least one version");
        let v = &versions[0];
        println!(
            "sodium@1.21.11/fabric: version={}, files={}",
            v.version_number,
            v.files.len()
        );
    }

    #[tokio::test]
    async fn test_guess_slug() {
        assert_eq!(
            guess_slug_from_filename("sodium-fabric-0.8.7+mc1.21.11.jar"),
            Some("sodium".into())
        );
        assert_eq!(
            guess_slug_from_filename("fabric-api-0.141.3+1.21.11.jar"),
            Some("fabric-api".into())
        );
        assert_eq!(guess_slug_from_filename("unknown-mod-1.0.0.jar"), None);
    }
}
