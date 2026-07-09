mod cli;
mod config;
mod discovery;
mod executor;
mod jar_inspector;
mod lockfile;
mod modrinth;
mod reconcile;
mod types;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Commands};
use crate::lockfile::Lockfile;
use crate::types::InstanceSource;

fn find_minecraft_dir(cli_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = cli_dir {
        if dir.exists() {
            return Ok(dir);
        }
        anyhow::bail!(
            "specified Minecraft directory does not exist: {}",
            dir.display()
        );
    }
    discovery::find_minecraft_dir()
}

fn find_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("unable to determine home directory")?;
    Ok(home.join(".config/mc"))
}

fn auto_detect_version(state: &types::MinecraftState) -> String {
    // Look at all mods' depends.minecraft fields to detect the version
    let mut versions: Vec<&str> = Vec::new();
    for inst in &state.instances {
        for m in &inst.mods {
            if let Some(ref meta) = m.metadata {
                if let Some(mc_constraint) = meta.depends.get("minecraft") {
                    // Extract version from constraints like >=1.21.11, ~1.21.11, 1.21.11
                    let v =
                        mc_constraint.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '.');
                    if !v.is_empty() && v.starts_with(|c: char| c.is_ascii_digit()) {
                        versions.push(v);
                    }
                }
            }
        }
    }

    // Find the most common version
    if versions.is_empty() {
        return "1.21.11".to_string();
    }

    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for v in &versions {
        *counts.entry(v).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(v, _)| v.to_string())
        .unwrap_or_else(|| "1.21.11".to_string())
}

fn auto_detect_loader(state: &types::MinecraftState) -> String {
    // Check the depends.fabricloader or depends.forgeloader field in mods
    for inst in &state.instances {
        for m in &inst.mods {
            if let Some(ref meta) = m.metadata {
                if meta.depends.contains_key("fabricloader") {
                    return "fabric".to_string();
                }
                if meta.depends.contains_key("forge") {
                    return "forge".to_string();
                }
            }
        }
    }
    "fabric".to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Status => cmd_status().await?,
        Commands::Init {
            minecraft_dir,
            force,
        } => cmd_init(minecraft_dir, force).await?,
        Commands::Plan { instance } => cmd_plan(instance).await?,
        Commands::Apply { instance } => cmd_apply(instance).await?,
        Commands::Update => cmd_update().await?,
        Commands::Add { r#mod, instance } => cmd_add(&r#mod, &instance).await?,
        Commands::Remove { r#mod, instance } => cmd_remove(&r#mod, &instance).await?,
        Commands::List { instance } => cmd_list(instance.as_deref()).await?,
        Commands::Search { query } => cmd_search(&query).await?,
        Commands::Info { r#mod } => cmd_info(&r#mod).await?,
        Commands::Rename { old, new, instance } => cmd_rename(&old, &new, &instance).await?,
    }

    Ok(())
}

async fn cmd_status() -> Result<()> {
    let mc_dir = find_minecraft_dir(None)?;
    let state = discovery::scan_minecraft(&mc_dir)?;
    let config_dir = find_config_dir()?;
    let lockfile = Lockfile::from_file(&config_dir.join("mcmod.lock.json"))?;

    println!("Minecraft directory: {}", state.minecraft_dir.display());
    println!("Instances: {}", state.instances.len());
    println!();

    for instance in &state.instances {
        let source_label = match instance.source {
            InstanceSource::Default => "default",
            InstanceSource::Named => "named",
        };
        println!(
            "  [{source_label}] {}  (mods: {})",
            instance.name,
            instance.mods_dir.display()
        );

        if instance.mods.is_empty() {
            println!("    (empty)");
        } else {
            for m in &instance.mods {
                let version = m
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.version.as_deref())
                    .unwrap_or("?");
                let name = m
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.name.as_deref())
                    .unwrap_or(&m.filename);

                // Check if managed
                let path_str = m.path.to_string_lossy().to_string();
                let managed = lockfile
                    .as_ref()
                    .and_then(|lf| lf.managed_paths.get(&path_str))
                    .map(|s| s.as_str())
                    .unwrap_or("?");

                let status = if managed != "?" { "✓" } else { " " };
                println!("    {status} {name:<40} v{version:<20} {}", &m.sha1[..12]);
            }
        }
        println!();
    }

    Ok(())
}

async fn cmd_init(minecraft_dir: Option<PathBuf>, force: bool) -> Result<()> {
    let mc_dir = find_minecraft_dir(minecraft_dir)?;
    let state = discovery::scan_minecraft(&mc_dir)?;

    if state.instances.is_empty() {
        anyhow::bail!("no instances or mod directories found");
    }

    let detected_version = auto_detect_version(&state);
    let detected_loader = auto_detect_loader(&state);

    println!("Scanned {} instance(s)", state.instances.len());
    println!("Detected Minecraft: {detected_version}, Loader: {detected_loader}");
    println!();

    for inst in &state.instances {
        println!("  {} ({} mods)", inst.name, inst.mods.len());
    }
    println!();

    let config_dir = find_config_dir()?;
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("mcmod.yaml");
    if config_path.exists() && !force {
        anyhow::bail!(
            "config already exists at {} (use --force to overwrite)",
            config_path.display()
        );
    }

    let mut config_instances: std::collections::HashMap<String, config::InstanceConfig> =
        std::collections::HashMap::new();

    for inst in &state.instances {
        let mods: Vec<config::ModRef> = inst
            .mods
            .iter()
            .filter_map(|m| {
                let slug = m
                    .metadata
                    .as_ref()
                    .map(|meta| meta.id.clone())
                    .or_else(|| modrinth::guess_slug_from_filename(&m.filename));
                slug.map(config::ModRef::Simple)
            })
            .collect();

        config_instances.insert(
            inst.name.clone(),
            config::InstanceConfig {
                minecraft_version: None,
                loader: None,
                mods: mods.clone(),
            },
        );
    }

    let config = config::Config {
        minecraft_version: detected_version,
        loader: detected_loader,
        instances: config_instances,
        mod_sources: std::collections::HashMap::new(),
    };

    config.to_file(&config_path)?;

    // Also create an initial lockfile
    let lockfile_path = config_dir.join("mcmod.lock.json");
    let lockfile = lockfile::Lockfile {
        version: 1,
        minecraft_version: config.minecraft_version.clone(),
        loader: config.loader.clone(),
        generated_at: chrono_now(),
        instances: std::collections::HashMap::new(),
        managed_paths: std::collections::HashMap::new(),
    };
    lockfile.to_file(&lockfile_path)?;

    println!("Config written to: {}", config_path.display());
    println!("Lockfile written to: {}", lockfile_path.display());
    println!();
    println!("Next steps:");
    println!("  1. Edit {config_path:?} to declare your mods");
    println!("  2. Run: mcmod update # resolve versions from Modrinth");
    println!("  3. Run: mcmod plan   # see what would change");
    println!("  4. Run: mcmod apply  # install/update/remove mods");

    Ok(())
}

async fn cmd_plan(instance_filter: Option<String>) -> Result<()> {
    let mc_dir = find_minecraft_dir(None)?;
    let config_dir = find_config_dir()?;
    let config_path = config_dir.join("mcmod.yaml");
    let lockfile_path = config_dir.join("mcmod.lock.json");

    let config = config::Config::from_file(&config_path).map_err(|_| {
        anyhow::anyhow!("no config found at {config_path:?}. Run 'mcmod init' first")
    })?;
    let lockfile = Lockfile::from_file(&lockfile_path)?;
    let mut state = discovery::scan_minecraft(&mc_dir)?;

    if let Some(ref filter) = instance_filter {
        state.instances.retain(|i| i.name == *filter);
        if state.instances.is_empty() {
            anyhow::bail!("instance '{filter}' not found");
        }
    }

    let plan = reconcile::compute_plan(&config, &lockfile, &state)?;
    plan.print();
    Ok(())
}

async fn cmd_apply(instance_filter: Option<String>) -> Result<()> {
    let mc_dir = find_minecraft_dir(None)?;
    let config_dir = find_config_dir()?;
    let config_path = config_dir.join("mcmod.yaml");
    let lockfile_path = config_dir.join("mcmod.lock.json");

    let config = config::Config::from_file(&config_path).map_err(|_| {
        anyhow::anyhow!("no config found at {config_path:?}. Run 'mcmod init' first")
    })?;
    let mut lockfile = Lockfile::from_file(&lockfile_path)?.unwrap_or_else(|| Lockfile {
        version: 1,
        minecraft_version: config.minecraft_version.clone(),
        loader: config.loader.clone(),
        generated_at: chrono_now(),
        instances: std::collections::HashMap::new(),
        managed_paths: std::collections::HashMap::new(),
    });

    let mut state = discovery::scan_minecraft(&mc_dir)?;
    if let Some(ref filter) = instance_filter {
        state.instances.retain(|i| i.name == *filter);
        if state.instances.is_empty() {
            anyhow::bail!("instance '{filter}' not found");
        }
    }

    let plan = reconcile::compute_plan(&config, &Some(lockfile.clone()), &state)?;

    if plan.is_empty() {
        println!("Nothing to do — everything is up to date.");
        return Ok(());
    }

    plan.print();
    println!();

    // Execute
    executor::execute_plan(&plan, &config, false).await?;

    // Update lockfile: remove stale paths, keep existing managed files
    lockfile
        .managed_paths
        .retain(|path_str, _| std::path::Path::new(path_str).exists());
    // Re-scan and add any new managed files that match desired slugs
    let state_after = discovery::scan_minecraft(&mc_dir)?;
    for inst in &state_after.instances {
        let mut wanted: Vec<String> = Vec::new();
        if let Some(cfg) = config.instances.get(&inst.name) {
            wanted.extend(cfg.mods.iter().filter_map(|m| m.slug()).map(String::from));
        }
        for m in &inst.mods {
            let path_str = m.path.to_string_lossy().to_string();
            if lockfile.managed_paths.contains_key(&path_str) {
                continue;
            }
            let slug = modrinth::guess_slug_from_filename(&m.filename);
            if let Some(ref s) = slug {
                if wanted.contains(s) {
                    lockfile.managed_paths.insert(path_str, s.clone());
                }
            }
        }
    }

    lockfile.generated_at = chrono_now();
    lockfile.to_file(&lockfile_path)?;
    println!("\nLockfile updated at: {}", lockfile_path.display());
    println!("Done.");
    Ok(())
}

async fn cmd_update() -> Result<()> {
    let config_dir = find_config_dir()?;
    let config_path = config_dir.join("mcmod.yaml");
    let lockfile_path = config_dir.join("mcmod.lock.json");

    let config = config::Config::from_file(&config_path).map_err(|_| {
        anyhow::anyhow!("no config found at {config_path:?}. Run 'mcmod init' first")
    })?;
    let mut lockfile = Lockfile::from_file(&lockfile_path)?.unwrap_or_else(|| {
        println!("No lockfile found — creating new one.");
        Lockfile {
            version: 1,
            minecraft_version: config.minecraft_version.clone(),
            loader: config.loader.clone(),
            generated_at: chrono_now(),
            instances: std::collections::HashMap::new(),
            managed_paths: std::collections::HashMap::new(),
        }
    });

    println!("Re-resolving all mod versions from Modrinth...");
    println!();

    for (name, inst_cfg) in &config.instances {
        let v = inst_cfg.effective_version(&config.minecraft_version);
        let l = inst_cfg.effective_loader(&config.loader);
        for m in &inst_cfg.mods {
            if let Some(slug) = m.slug() {
                println!("  {slug} ({name}, {v})...");
                resolve_and_store(&mut lockfile, name, slug, v, l, &config).await;
            }
        }
    }

    lockfile.generated_at = chrono_now();
    lockfile.to_file(&lockfile_path)?;
    println!("\nLockfile updated at: {}", lockfile_path.display());
    Ok(())
}

async fn resolve_and_store(
    lockfile: &mut lockfile::Lockfile,
    instance: &str,
    slug: &str,
    mc_version: &str,
    loader: &str,
    config: &config::Config,
) {
    match executor::resolve_mod(slug, mc_version, loader, config).await {
        Ok((url, sha512, filename)) => {
            let inst = lockfile
                .instances
                .entry(instance.to_string())
                .or_insert_with(|| lockfile::LockedInstance {
                    mods: std::collections::HashMap::new(),
                });
            inst.mods.insert(
                slug.to_string(),
                lockfile::LockedMod {
                    project_id: String::new(),
                    slug: slug.to_string(),
                    version_id: String::new(),
                    version_number: String::new(),
                    filename,
                    sha512,
                    download_url: url,
                    dependencies: Vec::new(),
                },
            );
            println!("    ✓ resolved");
        }
        Err(e) => {
            println!("    ⚠ failed to resolve: {e:?}");
        }
    }
}

// --- New command handlers ---

fn config_path() -> anyhow::Result<std::path::PathBuf> {
    let config_dir = find_config_dir()?;
    Ok(config_dir.join("mcmod.yaml"))
}

async fn cmd_add(mod_slug: &str, instance: &str) -> Result<()> {
    let config_path = config_path()?;
    if !config_path.exists() {
        anyhow::bail!(
            "no config found at {:?}. Run 'mcmod init' first",
            config_path
        );
    }
    let mut config = config::Config::from_file(&config_path)?;

    let inst = config
        .instances
        .entry(instance.to_string())
        .or_insert_with(|| config::InstanceConfig {
            minecraft_version: None,
            loader: None,
            mods: Vec::new(),
        });

    // Check for duplicates
    if inst.mods.iter().any(|m| m.slug() == Some(mod_slug)) {
        println!("  ✓ {mod_slug} already in '{instance}'");
        return Ok(());
    }

    inst.mods.push(config::ModRef::Simple(mod_slug.to_string()));
    config.to_file(&config_path)?;
    println!("  ✓ Added {mod_slug} to '{instance}'");
    println!();
    println!("  Next: run 'mcmod update' to resolve versions, then 'mcmod apply'");

    Ok(())
}

async fn cmd_remove(mod_slug: &str, instance: &str) -> Result<()> {
    let config_path = config_path()?;
    let mut config = config::Config::from_file(&config_path)?;

    let inst = config
        .instances
        .get_mut(instance)
        .ok_or_else(|| anyhow::anyhow!("instance '{instance}' not found in config"))?;

    let before = inst.mods.len();
    inst.mods.retain(|m| m.slug() != Some(mod_slug));

    if inst.mods.len() == before {
        anyhow::bail!("mod '{mod_slug}' not found in instance '{instance}'");
    }

    config.to_file(&config_path)?;
    println!("  ✓ Removed {mod_slug} from '{instance}'");
    println!();
    println!("  Next: run 'mcmod apply' to remove from disk");

    Ok(())
}

async fn cmd_list(instance_filter: Option<&str>) -> Result<()> {
    let config_dir = find_config_dir()?;
    let config_path = config_dir.join("mcmod.yaml");
    let lockfile_path = config_dir.join("mcmod.lock.json");

    if !config_path.exists() {
        anyhow::bail!("no config found at {:?}", config_path);
    }

    let config = config::Config::from_file(&config_path)?;
    let lockfile = lockfile::Lockfile::from_file(&lockfile_path)?;

    for (name, inst_cfg) in &config.instances {
        if let Some(filter) = instance_filter {
            if name != filter {
                continue;
            }
        }

        let version = inst_cfg.effective_version(&config.minecraft_version);
        let loader = inst_cfg.effective_loader(&config.loader);
        println!("[{name}]  MC: {version}, Loader: {loader}");
        println!("  Mods ({}):", inst_cfg.mods.len());

        if inst_cfg.mods.is_empty() {
            println!("    (none configured)");
        } else {
            for m in &inst_cfg.mods {
                let slug = m.slug().unwrap_or("?");
                let resolved = lockfile
                    .as_ref()
                    .and_then(|lf| lf.mod_for(name, slug))
                    .map(|lm| format!("v{}", lm.version_number))
                    .unwrap_or_else(|| "unresolved".to_string());
                println!("    {slug:<30} {resolved}");
            }
        }
        println!();
    }

    Ok(())
}

async fn cmd_search(query: &str) -> Result<()> {
    println!("Searching Modrinth for '{query}'...");
    println!();

    let hits = crate::modrinth::search_projects(query).await?;

    if hits.is_empty() {
        println!("  No results found.");
        return Ok(());
    }

    println!("  Results ({}/{} shown):", hits.len(), hits.len());
    println!();
    for hit in &hits {
        let desc = hit
            .description
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        println!("  {:<25} {}", hit.slug, hit.title);
        if !desc.is_empty() {
            println!("  {:25} {}", "", desc);
        }
        println!(
            "  {:25} ⬇ {}  ★ {}  [{:.2}]",
            "", hit.downloads, hit.follows, hit.project_type
        );
        println!();
    }

    Ok(())
}

async fn cmd_info(mod_slug: &str) -> Result<()> {
    println!("Fetching info for '{mod_slug}' from Modrinth...");
    println!();

    let project = crate::modrinth::get_project(mod_slug).await?;
    let versions = crate::modrinth::get_all_versions(&project.id)
        .await
        .unwrap_or_default();

    println!("  Slug:        {}", project.slug);
    println!("  Title:       {}", project.title);
    println!("  ID:          {}", project.id);
    if let Some(desc) = &project.description {
        println!("  Description: {}", desc);
    }
    println!();
    println!("  Versions ({} total):", versions.len());

    for v in versions.iter().take(10) {
        let games = v.game_versions.join(", ");
        let loaders = v.loaders.join(", ");
        println!(
            "    v{:<15} [{:.8}] {} | {}",
            v.version_number,
            v.version_type,
            if games.is_empty() { "?" } else { &games },
            loaders
        );
    }

    if versions.len() > 10 {
        println!("    ... and {} more", versions.len() - 10);
    }

    Ok(())
}

async fn cmd_rename(old: &str, new: &str, instance: &str) -> Result<()> {
    let config_path = config_path()?;
    let mut config = config::Config::from_file(&config_path)?;

    let inst = config
        .instances
        .get_mut(instance)
        .ok_or_else(|| anyhow::anyhow!("instance '{instance}' not found in config"))?;

    let mut found = false;
    for m in &mut inst.mods {
        if let config::ModRef::Simple(s) = m {
            if s == old {
                *s = new.to_string();
                found = true;
                break;
            }
        }
        if let config::ModRef::Detailed { slug, .. } = m {
            if slug.as_deref() == Some(old) {
                *slug = Some(new.to_string());
                found = true;
                break;
            }
        }
    }

    if !found {
        anyhow::bail!("mod '{old}' not found in instance '{instance}'");
    }

    config.to_file(&config_path)?;
    println!("  ✓ Renamed '{old}' → '{new}' in '{instance}'");
    println!();
    println!("  Next: run 'mcmod update' then 'mcmod apply'");

    Ok(())
}

fn chrono_now() -> String {
    // Simple ISO-8601 timestamp without chrono crate
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Format: 2026-07-09T12:00:00Z
    let days_since_epoch = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Calculate date from days since epoch (1970-01-01)
    // Use a simple algorithm
    let mut y = 1970i64;
    let mut remaining = days_since_epoch as i64;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let months_days: &[(i64, i64)] = if is_leap(y) {
        &[
            (31, 0),
            (29, 31),
            (31, 60),
            (30, 91),
            (31, 121),
            (30, 152),
            (31, 182),
            (31, 213),
            (30, 244),
            (31, 274),
            (30, 305),
            (31, 335),
        ]
    } else {
        &[
            (31, 0),
            (28, 31),
            (31, 59),
            (30, 90),
            (31, 120),
            (30, 151),
            (31, 181),
            (31, 212),
            (30, 243),
            (31, 273),
            (30, 304),
            (31, 334),
        ]
    };

    let mut month = 1;
    let mut day = remaining + 1;
    for (m_days, _) in months_days {
        if day <= *m_days {
            break;
        }
        day -= m_days;
        month += 1;
        if month > 12 {
            break;
        }
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, month, day, hours, minutes, seconds
    )
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
