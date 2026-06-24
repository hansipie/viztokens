use std::collections::HashSet;

use crate::model::MessageType;

#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub allowed_types: HashSet<MessageType>,
}

impl MessageFilter {
    pub fn is_active(&self) -> bool {
        !self.allowed_types.is_empty()
    }

    pub fn allows(&self, t: &MessageType) -> bool {
        !self.is_active() || self.allowed_types.contains(t)
    }

    pub fn toggle(&mut self, t: MessageType) {
        if !self.allowed_types.remove(&t) {
            self.allowed_types.insert(t);
        }
    }

    pub fn clear(&mut self) {
        self.allowed_types.clear();
    }

    pub fn label(&self) -> &str {
        if !self.is_active() {
            return "ALL";
        }
        if self.allowed_types.len() == 1 {
            return match self.allowed_types.iter().next().unwrap() {
                MessageType::System => "SYSTEM",
                MessageType::User => "USER",
                MessageType::Assistant => "ASSISTANT",
                MessageType::ToolCall => "TOOLS",
                MessageType::ToolResult => "TOOLS",
            };
        }
        // Check if exactly ToolCall+ToolResult
        if self.allowed_types.len() == 2
            && self.allowed_types.contains(&MessageType::ToolCall)
            && self.allowed_types.contains(&MessageType::ToolResult)
        {
            return "TOOLS";
        }
        "CUSTOM"
    }
}
