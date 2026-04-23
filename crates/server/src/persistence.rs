//! On-disk persistence for device types, contexts and app settings.
//!
//! Layout (under `<config-dir>/modbus-simulator/`):
//!   - `settings.json`
//!   - `active.json`
//!   - `device-types/<uuid>.json`
//!   - `contexts/<uuid>.json`
//!   - `virtual-serials.json` — persisted list of PTY intents (id + symlink).
//!     PTY master fds can't survive a restart, but we re-create a fresh
//!     PTY pair on startup for each intent, keeping the id and symlink
//!     stable so RTU transport config references (and the user's test app
//!     which opened the symlink) stay valid.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use directories::ProjectDirs;
use modsim_core::model::{Context, ContextId, DeviceType, DeviceTypeId, World};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_http_bind")]
    pub http_bind: String,
}

fn default_http_port() -> u16 {
    8080
}

fn default_http_bind() -> String {
    "127.0.0.1".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            http_port: default_http_port(),
            http_bind: default_http_bind(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ActiveFile {
    context_id: Option<ContextId>,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new_default() -> Result<Self> {
        // Explicit override (useful for tests and ad-hoc runs with isolated state).
        if let Ok(override_path) = std::env::var("MODSIM_CONFIG_DIR") {
            return Self::with_root(PathBuf::from(override_path));
        }
        let dirs = ProjectDirs::from("dev", "emcell", "modbus-simulator")
            .context("unable to determine config dir")?;
        let root = dirs.config_dir().to_path_buf();
        Self::with_root(root)
    }

    pub fn with_root(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        fs::create_dir_all(root.join("device-types"))?;
        fs::create_dir_all(root.join("contexts"))?;
        Ok(Self { root })
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    fn active_path(&self) -> PathBuf {
        self.root.join("active.json")
    }
    fn device_type_path(&self, id: DeviceTypeId) -> PathBuf {
        self.root.join("device-types").join(format!("{id}.json"))
    }
    fn context_path(&self, id: ContextId) -> PathBuf {
        self.root.join("contexts").join(format!("{id}.json"))
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        let p = self.settings_path();
        if !p.exists() {
            let s = AppSettings::default();
            self.save_settings(&s)?;
            return Ok(s);
        }
        let s = fs::read_to_string(&p)?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        atomic_write(&self.settings_path(), &serde_json::to_vec_pretty(s)?)
    }

    pub fn load_world(&self) -> Result<World> {
        let mut world = World::default();
        // device types
        let dt_dir = self.root.join("device-types");
        for entry in fs::read_dir(&dt_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let s = fs::read_to_string(&path)?;
                let dt: DeviceType = serde_json::from_str(&s)
                    .with_context(|| format!("parse {}", path.display()))?;
                world.device_types.push(dt);
            }
        }
        // contexts
        let ctx_dir = self.root.join("contexts");
        for entry in fs::read_dir(&ctx_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let s = fs::read_to_string(&path)?;
                let c: Context = serde_json::from_str(&s)
                    .with_context(|| format!("parse {}", path.display()))?;
                world.contexts.push(c);
            }
        }
        // active
        let active_path = self.active_path();
        if active_path.exists() {
            let s = fs::read_to_string(&active_path)?;
            let a: ActiveFile = serde_json::from_str(&s).unwrap_or_default();
            world.active_context_id = a.context_id;
        }
        Ok(world)
    }

    pub fn save_device_type(&self, dt: &DeviceType) -> Result<()> {
        atomic_write(
            &self.device_type_path(dt.id),
            &serde_json::to_vec_pretty(dt)?,
        )
    }

    pub fn delete_device_type(&self, id: DeviceTypeId) -> Result<()> {
        let p = self.device_type_path(id);
        if p.exists() {
            fs::remove_file(p)?;
        }
        Ok(())
    }

    pub fn save_context(&self, c: &Context) -> Result<()> {
        atomic_write(&self.context_path(c.id), &serde_json::to_vec_pretty(c)?)
    }

    pub fn delete_context(&self, id: ContextId) -> Result<()> {
        let p = self.context_path(id);
        if p.exists() {
            fs::remove_file(p)?;
        }
        Ok(())
    }

    pub fn save_active(&self, id: Option<ContextId>) -> Result<()> {
        atomic_write(
            &self.active_path(),
            &serde_json::to_vec_pretty(&ActiveFile { context_id: id })?,
        )
    }

    fn virtual_serials_path(&self) -> PathBuf {
        self.root.join("virtual-serials.json")
    }

    pub fn load_virtual_serials(&self) -> Result<Vec<VirtualSerialIntent>> {
        let p = self.virtual_serials_path();
        if !p.exists() {
            return Ok(Vec::new());
        }
        let s = fs::read_to_string(&p)?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    }

    pub fn save_virtual_serials(&self, intents: &[VirtualSerialIntent]) -> Result<()> {
        atomic_write(
            &self.virtual_serials_path(),
            &serde_json::to_vec_pretty(intents)?,
        )
    }
}

/// Persistent description of a virtual serial port. The `id` is re-used
/// verbatim after a restart so RTU configs referencing it keep working.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualSerialIntent {
    pub id: String,
    pub symlink_path: Option<PathBuf>,
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}
