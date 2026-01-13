use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, DogAfterHook, HookContext};
use serde_json::Value;
use crate::services::types::FleetParams;

pub struct BeforeRead;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate delivery data before read
        Ok(())
    }
}

pub struct AfterRead;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterRead {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Post-process delivery data after read
        Ok(())
    }
}

pub struct BeforeWrite;

#[async_trait]
impl DogBeforeHook<Value, FleetParams> for BeforeWrite {
    async fn run(&self, _ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Validate delivery data before creation/update
        if let Some(delivery_data) = _ctx.data.as_ref().and_then(|v| v.as_object()) {
            // Validate required fields
            if !delivery_data.contains_key("pickup-address") {
                return Err(anyhow::anyhow!("Pickup address is required"));
            }
            if !delivery_data.contains_key("delivery-address") {
                return Err(anyhow::anyhow!("Delivery address is required"));
            }
        }
        Ok(())
    }
}

pub struct AfterWrite;

#[async_trait]
impl DogAfterHook<Value, FleetParams> for AfterWrite {
    async fn run(&self, ctx: &mut HookContext<Value, FleetParams>) -> Result<()> {
        // Trigger TomTom operations after delivery creation/update
        if let Some(delivery_data) = ctx.data.as_ref().and_then(|v| v.as_object()) {
            let delivery_id = delivery_data.get("delivery-id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            
            let pickup_address = delivery_data.get("pickup-address")
                .and_then(|v| v.as_str())
                .unwrap_or("");
                
            let delivery_address = delivery_data.get("delivery-address")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            println!("🚚 Delivery {} created/updated - Triggering TomTom workflow", delivery_id);
            
            // In a real implementation, we would:
            // 1. Get TomTomQueueService from context or service registry
            // 2. Queue geocoding jobs for addresses
            // 3. Queue route calculation
            // 4. Queue initial traffic check
            
            // For now, just log the intended actions
            if !pickup_address.is_empty() {
                println!("  → Would queue geocoding for pickup: {}", pickup_address);
            }
            if !delivery_address.is_empty() {
                println!("  → Would queue geocoding for delivery: {}", delivery_address);
            }
            println!("  → Would queue route calculation for delivery {}", delivery_id);
        }

        Ok(())
    }
}
