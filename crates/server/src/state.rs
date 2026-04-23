//! Shared application state.

use std::sync::Arc;

use modsim_core::model::{Context, ContextId, Device, DeviceType, DeviceTypeId, World};
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::persistence::{AppSettings, Store};
use crate::transport::vpty::PtyRegistry;

#[derive(Debug, Clone)]
pub enum WorldEvent {
    WorldChanged,
    TrafficFrame(TrafficFrame),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrafficFrame {
    pub direction: &'static str,
    pub transport: &'static str,
    pub slave_id: u8,
    pub function_code: u8,
    pub bytes_hex: String,
    pub timestamp_ms: u128,
}

pub struct AppState {
    pub world: RwLock<World>,
    pub settings: RwLock<AppSettings>,
    pub store: Store,
    pub events: broadcast::Sender<WorldEvent>,
    pub ptys: Arc<PtyRegistry>,
}

impl AppState {
    pub fn new(world: World, settings: AppSettings, store: Store) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(256);
        Arc::new(Self {
            world: RwLock::new(world),
            settings: RwLock::new(settings),
            store,
            events: tx,
            ptys: PtyRegistry::new(),
        })
    }

    pub fn notify(&self, ev: WorldEvent) {
        let _ = self.events.send(ev);
    }

    pub fn snapshot(&self) -> World {
        self.world.read().clone()
    }

    /// Find device + device type + effective behavior for a slave id.
    #[allow(clippy::type_complexity)]
    pub fn resolve_slave(&self, slave_id: u8) -> Option<(Device, DeviceType)> {
        let w = self.world.read();
        let ctx = w.active_context()?;
        let dev = ctx.devices.iter().find(|d| d.slave_id == slave_id)?.clone();
        let dt = w.device_type(dev.device_type_id)?.clone();
        Some((dev, dt))
    }

    pub fn save_world_full(&self) -> anyhow::Result<()> {
        let w = self.world.read();
        for dt in &w.device_types {
            self.store.save_device_type(dt)?;
        }
        for c in &w.contexts {
            self.store.save_context(c)?;
        }
        self.store.save_active(w.active_context_id)?;
        Ok(())
    }

    pub fn save_device_type(&self, id: DeviceTypeId) -> anyhow::Result<()> {
        let w = self.world.read();
        if let Some(dt) = w.device_type(id) {
            self.store.save_device_type(dt)?;
        }
        Ok(())
    }

    pub fn save_context(&self, id: ContextId) -> anyhow::Result<()> {
        let w = self.world.read();
        if let Some(c) = w.contexts.iter().find(|c| c.id == id) {
            self.store.save_context(c)?;
        }
        Ok(())
    }

    pub fn save_active(&self) -> anyhow::Result<()> {
        let id = self.world.read().active_context_id;
        self.store.save_active(id)
    }

    pub fn apply_device_update<F: FnOnce(&mut Context)>(&self, f: F) {
        let mut w = self.world.write();
        if let Some(ctx) = w.active_context_mut() {
            f(ctx);
        }
    }
}
