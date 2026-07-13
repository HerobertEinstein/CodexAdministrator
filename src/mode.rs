use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    GrokNativeModel,
    NativeGptMain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeState {
    pub mode: AgentMode,
    pub revision: u64,
    pub task_id: Option<String>,
}

impl Default for ModeState {
    fn default() -> Self {
        Self {
            mode: AgentMode::NativeGptMain,
            revision: 0,
            task_id: None,
        }
    }
}

impl ModeState {
    pub fn set_mode(&mut self, mode: AgentMode) {
        if self.mode != mode {
            self.mode = mode;
            self.revision = self.revision.saturating_add(1);
        }
    }

    pub fn link_task(&mut self, task_id: &str) -> Result<()> {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            bail!("task id cannot be blank");
        }
        if task_id.len() > 128 {
            bail!("task id cannot exceed 128 bytes");
        }
        if self.task_id.as_deref() != Some(task_id) {
            self.task_id = Some(task_id.to_owned());
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }
}
