//! Shared application state.

use std::collections::HashMap;
use std::sync::Arc;

use modsim_core::model::{
    Context, ContextId, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, World,
};
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::persistence::{AppSettings, Store};
use crate::supervisor::TransportSupervisor;
use crate::transport::vpty::PtyRegistry;

#[derive(Debug, Clone)]
pub enum WorldEvent {
    WorldChanged,
    TrafficFrame(TrafficFrame),
}

#[derive(Debug, Clone)]
pub struct TrafficFrame {
    pub direction: &'static str,
    pub transport: &'static str,
    pub slave_id: u8,
    pub function_code: u8,
    pub bytes_hex: String,
    pub timestamp_ms: u128,
    /// Human-readable one-liner (e.g. "Read Holding Registers start=0 len=2").
    pub summary: String,
    /// Decoded values matched against the device's register definitions.
    pub decoded: Vec<DecodedValue>,
}

#[derive(Debug, Clone)]
pub struct DecodedValue {
    pub register_name: String,
    pub address: u16,
    pub data_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceActivity {
    pub last_read_at_ms: Option<u128>,
    pub last_write_at_ms: Option<u128>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RegisterActivity {
    pub last_read_at_ms: Option<u128>,
    pub last_write_at_ms: Option<u128>,
}

pub struct AppState {
    pub world: RwLock<World>,
    pub settings: RwLock<AppSettings>,
    pub store: Store,
    pub events: broadcast::Sender<WorldEvent>,
    pub ptys: Arc<PtyRegistry>,
    pub supervisor: TransportSupervisor,
    pub activity: RwLock<HashMap<DeviceId, DeviceActivity>>,
    pub register_activity: RwLock<HashMap<(DeviceId, RegisterId), RegisterActivity>>,
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
            supervisor: TransportSupervisor::new(),
            activity: RwLock::new(HashMap::new()),
            register_activity: RwLock::new(HashMap::new()),
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

    /// Apply the current active context's transport settings live — stops
    /// previous TCP/RTU listeners and spawns fresh ones. Safe to call
    /// repeatedly from mutation handlers.
    pub async fn reconfigure_transports(self: &Arc<Self>) {
        self.supervisor.reconfigure(self).await;
    }

    pub fn mark_device_read(&self, id: DeviceId, timestamp_ms: u128) {
        self.activity.write().entry(id).or_default().last_read_at_ms = Some(timestamp_ms);
    }

    pub fn mark_device_write(&self, id: DeviceId, timestamp_ms: u128) {
        self.activity
            .write()
            .entry(id)
            .or_default()
            .last_write_at_ms = Some(timestamp_ms);
    }

    pub fn device_activity(&self, id: DeviceId) -> DeviceActivity {
        self.activity.read().get(&id).copied().unwrap_or_default()
    }

    pub fn mark_register_read(&self, dev: DeviceId, reg: RegisterId, timestamp_ms: u128) {
        self.register_activity
            .write()
            .entry((dev, reg))
            .or_default()
            .last_read_at_ms = Some(timestamp_ms);
    }

    pub fn mark_register_write(&self, dev: DeviceId, reg: RegisterId, timestamp_ms: u128) {
        self.register_activity
            .write()
            .entry((dev, reg))
            .or_default()
            .last_write_at_ms = Some(timestamp_ms);
    }

    pub fn register_activity_for_device(
        &self,
        dev: DeviceId,
    ) -> Vec<(RegisterId, RegisterActivity)> {
        self.register_activity
            .read()
            .iter()
            .filter(|((d, _), _)| *d == dev)
            .map(|((_, r), a)| (*r, *a))
            .collect()
    }

    /// Persist the current list of virtual serials (id + optional symlink)
    /// so they can be re-created on next startup.
    pub fn save_virtual_serials(&self) -> anyhow::Result<()> {
        let intents: Vec<crate::persistence::VirtualSerialIntent> = self
            .ptys
            .list()
            .into_iter()
            .map(|v| crate::persistence::VirtualSerialIntent {
                id: v.id,
                symlink_path: v.symlink_path,
            })
            .collect();
        self.store.save_virtual_serials(&intents)
    }

    pub fn apply_device_update<F: FnOnce(&mut Context)>(&self, f: F) {
        let mut w = self.world.write();
        if let Some(ctx) = w.active_context_mut() {
            f(ctx);
        }
    }
}
