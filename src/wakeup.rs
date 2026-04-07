use crate::config::AppConfig;
use crate::layers::MemoryStack;
use crate::storage::Storage;
use anyhow::Result;

pub fn render_wakeup(config: &AppConfig, storage: &Storage, wing: Option<&str>) -> Result<String> {
    let stack = MemoryStack::new(config, storage, wing);
    stack.wake_up()
}
