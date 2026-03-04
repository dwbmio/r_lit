//! Application Model — pure data state, zero GUI dependency.
//!
//! All application state lives here. The controller mutates it,
//! the view reads it for rendering.

use crate::gui::{Toast, ToastQueue};
use crate::shared_file::SharedFile;
use crate::user_db::UserInfo;
use std::sync::Arc;

// ── State Enums ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Login,
    GroupDiscovery,
    Searching,
    GroupLobby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineStatus {
    Online,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Leader,
    Member,
}

// ── Component Models ────────────────────────────────────────────────

/// Avatar 的纯数据模型，每个需要展示头像的地方都复用这个结构
#[derive(Debug, Clone)]
pub struct AvatarModel {
    pub id: String,
    pub nickname: String,
    pub role: Role,
    pub status: OnlineStatus,
}

impl AvatarModel {
    pub fn new(id: String, nickname: String) -> Self {
        Self {
            id,
            nickname,
            role: Role::Member,
            status: OnlineStatus::Offline,
        }
    }

    pub fn is_online(&self) -> bool {
        self.status == OnlineStatus::Online
    }

    pub fn is_leader(&self) -> bool {
        self.role == Role::Leader
    }
}

// ── Page Models ─────────────────────────────────────────────────────

/// 群组大厅的纯数据模型
#[derive(Debug, Clone)]
pub struct GroupLobbyModel {
    pub group_id: String,
    pub me: AvatarModel,
    pub others: Vec<AvatarModel>,
}

impl GroupLobbyModel {
    pub fn new(group_id: String, me: AvatarModel) -> Self {
        Self {
            group_id,
            me,
            others: Vec::new(),
        }
    }

    pub fn add_member(&mut self, member: AvatarModel) {
        if member.id != self.me.id {
            self.others.push(member);
        }
    }

    pub fn remove_member(&mut self, member_id: &str) {
        self.others.retain(|m| m.id != member_id);
    }

    pub fn update_member_status(&mut self, member_id: &str, status: OnlineStatus) {
        if member_id == self.me.id {
            self.me.status = status;
        } else if let Some(m) = self.others.iter_mut().find(|m| m.id == member_id) {
            m.status = status;
        }
    }

    pub fn online_count(&self) -> usize {
        let me_count = if self.me.is_online() { 1 } else { 0 };
        me_count + self.others.iter().filter(|m| m.is_online()).count()
    }

    pub fn total_count(&self) -> usize {
        1 + self.others.len()
    }
}

// ── Data Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DiscoveredGroup {
    pub group_id: String,
    pub members: Vec<(String, String)>,
}

// ── Application Model ───────────────────────────────────────────────

pub struct AppModel {
    pub current_user: UserInfo,
    pub state: AppState,
    pub swarm_state: SwarmState,
    pub group_lobby: Option<GroupLobbyModel>,
    pub toast_queue: ToastQueue,
    pub shared_file: Option<Arc<SharedFile>>,
    pub should_close_loading: bool,
    pub discovered_groups: Vec<DiscoveredGroup>,
    pub search_error: Option<String>,
}

impl AppModel {
    pub fn new(current_user: UserInfo, show_login: bool) -> Self {
        let state = if show_login {
            AppState::Login
        } else {
            AppState::GroupDiscovery
        };

        Self {
            current_user,
            state,
            swarm_state: SwarmState::Disconnected,
            group_lobby: None,
            toast_queue: ToastQueue::new(),
            shared_file: None,
            should_close_loading: false,
            discovered_groups: Vec::new(),
            search_error: None,
        }
    }

    pub fn push_toast(&mut self, toast: Toast) {
        self.toast_queue.push(toast);
    }
}

// ── Async Results (sent from controller threads to view) ────────────

#[derive(Debug)]
pub enum SearchResult {
    Found {
        groups: Vec<DiscoveredGroup>,
    },
    Empty {
        peer_count: usize,
    },
    Error(String),
}

#[derive(Debug)]
pub enum JoinResult {
    Connected(usize),
    NoPeers,
    Error(String),
}

/// Periodic member list update from swarm
#[derive(Debug)]
pub struct MemberUpdate {
    pub members: Vec<AvatarModel>,
}
