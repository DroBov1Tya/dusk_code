//! What the user prefers: threads, limits, notifications, memory, speech, approvals and the like.

use super::Config;
use crate::config::{
    Appearance, ApprovalPolicy, Limits, Logging, Memory, ThreadConfig, VersionControl, WorkHistory,
};
use crate::release::update::Channel;
use dusk_protocol::config::VoiceSettings;

impl Config {
    pub fn thread(&self) -> &ThreadConfig {
        &self.thread
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn sound_notifications(&self) -> bool {
        self.sound_notifications
    }

    /// Whether clients notify at all.
    pub fn notifications(&self) -> bool {
        self.notifications
    }

    pub fn set_notifications(&mut self, on: bool) {
        self.notifications = on;
    }

    pub fn set_sound_notifications(&mut self, on: bool) {
        self.sound_notifications = on;
    }

    pub fn version_control(&self) -> &VersionControl {
        &self.version_control
    }

    pub fn set_branch_prefix(&mut self, prefix: &str) {
        self.version_control.branch_prefix = prefix.trim().to_owned();
    }

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn set_idle_screen_minutes(&mut self, minutes: u32) {
        self.appearance.idle_screen_minutes = minutes.min(Appearance::MAX_IDLE_SCREEN_MINUTES);
    }

    pub fn memory(&self) -> Memory {
        self.memory
    }

    pub fn set_memory_enabled(&mut self, enabled: bool) {
        self.memory.enabled = enabled;
    }

    pub fn set_memory_chars(&mut self, chars: usize) {
        self.memory.max_chars = chars;
    }

    pub fn work_history(&self) -> WorkHistory {
        self.work_history
    }

    pub fn set_work_history_enabled(&mut self, enabled: bool) {
        self.work_history.enabled = enabled;
    }

    pub fn set_work_history_chars(&mut self, chars: usize) {
        self.work_history.max_chars = chars;
    }

    pub fn update_channel(&self) -> Channel {
        self.update_channel.unwrap_or_default()
    }

    pub fn set_update_channel(&mut self, channel: Channel) {
        self.update_channel = (channel != Channel::Stable).then_some(channel);
    }

    pub fn voice(&self) -> &VoiceSettings {
        &self.voice
    }

    pub fn set_voice(&mut self, voice: VoiceSettings) {
        self.voice = voice;
    }

    pub fn approvals(&self) -> ApprovalPolicy {
        self.approvals
    }

    pub fn logging(&self) -> &Logging {
        &self.logging
    }

    pub fn set_logging_enabled(&mut self, enabled: bool) {
        self.logging.enabled = enabled;
    }

    pub fn set_approvals(&mut self, policy: ApprovalPolicy) {
        self.approvals = policy;
    }

    pub fn set_max_agents_at_work(&mut self, limit: usize) {
        self.limits.max_agents_at_work = limit;
    }

    pub fn set_max_goal_rounds(&mut self, rounds: u32) {
        self.limits.max_goal_rounds = rounds;
    }
}
