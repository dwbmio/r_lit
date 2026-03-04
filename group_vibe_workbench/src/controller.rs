//! Application Controller — business logic, model mutations, async tasks.
//!
//! All side effects (swarm operations, DB writes, file I/O) live here.
//! The view calls controller functions; they mutate the model and/or
//! return channels for async results.

use crate::config::Config;
use crate::gui::Toast;
use crate::model::*;
use crate::shared_file::SharedFile;
use crate::user_db::{UserDatabase, UserInfo};
use std::sync::{mpsc, Arc};

// ── Login ───────────────────────────────────────────────────────────

pub fn handle_login(model: &mut AppModel, user: UserInfo, config: &Config) {
    log::info!("User logged in: {} ({})", user.nickname, user.id);

    let db_path = config.user_db_path();
    match UserDatabase::open(db_path) {
        Ok(user_db) => {
            if let Err(e) = user_db.save_user(&user) {
                log::error!("Failed to save user: {:?}", e);
                model.push_toast(Toast::error("保存用户信息失败"));
            } else {
                model.push_toast(Toast::success(format!("欢迎, {}!", user.nickname)));
            }
        }
        Err(e) => {
            log::error!("Failed to open user db: {:?}", e);
        }
    }

    model.current_user = user;
    model.state = AppState::GroupDiscovery;
}

pub fn use_default_nickname(model: &mut AppModel, config: &Config) {
    let nickname = format!("User_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let user = UserInfo::new(nickname);
    handle_login(model, user, config);
}

pub fn validate_nickname(nickname: &str) -> Result<(), &'static str> {
    if nickname.is_empty() {
        return Err("请输入昵称");
    }
    if nickname.len() < 2 {
        return Err("昵称至少需要 2 个字符");
    }
    if nickname.len() > 20 {
        return Err("昵称不能超过 20 个字符");
    }
    Ok(())
}

// ── Search ──────────────────────────────────────────────────────────

pub fn start_search(
    model: &mut AppModel,
    config: &Config,
) -> mpsc::Receiver<SearchResult> {
    log::info!("Starting group search...");

    model.state = AppState::Searching;
    model.discovered_groups.clear();
    model.search_error = None;
    model.swarm_state = SwarmState::Connecting;

    let swarm_path = config.swarm_path(&model.current_user.id).join("_search");
    let (tx, rx) = mpsc::channel::<SearchResult>();

    std::thread::spawn(move || {
        let swarm_config = crate::swarm_manager::SwarmConfig {
            storage_path: swarm_path,
            group_id: "_search_".to_string(),
        };

        match crate::swarm_manager::get_or_init_swarm(swarm_config) {
            Ok(swarm) => {
                log::info!("Search swarm started, waiting for events...");
                let handle = crate::swarm_manager::spawn(async move {
                    let mut events = swarm.subscribe();

                    // murmur auto-discovery runs in background; just wait for events
                    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(8);
                    let mut got_sync = false;
                    loop {
                        tokio::select! {
                            _ = tokio::time::sleep_until(deadline) => break,
                            event = events.recv() => {
                                match event {
                                    Ok(murmur::SwarmEvent::DataSynced) => {
                                        log::info!("Search: data synced via event");
                                        got_sync = true;
                                        break;
                                    }
                                    Ok(murmur::SwarmEvent::PeerConnected { .. }) => {
                                        log::info!("Search: peer connected via event");
                                        // Wait briefly for CRDT sync after connection
                                        tokio::select! {
                                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(300)) => {}
                                            ev = events.recv() => {
                                                if let Ok(murmur::SwarmEvent::DataSynced) = ev {
                                                    log::info!("Search: data synced");
                                                    got_sync = true;
                                                }
                                            }
                                        }
                                        break;
                                    }
                                    Ok(murmur::SwarmEvent::PeerDisconnected { .. }) => continue,
                                    Err(_) => break,
                                }
                            }
                        }
                    }

                    // If we got a sync, or we have connections, read the group list
                    if !got_sync && !swarm.connected_peers().await.is_empty() {
                        // One more brief wait for CRDT data
                        tokio::select! {
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(200)) => {}
                            ev = events.recv() => {
                                if let Ok(murmur::SwarmEvent::DataSynced) = ev {
                                    log::info!("Search: late data synced");
                                }
                            }
                        }
                    }

                    let peers = swarm.list_announced_peers().await
                        .unwrap_or_default();
                    log::info!("Search: found {} announced peer(s)", peers.len());

                    let mut groups_map: std::collections::HashMap<String, Vec<(String, String)>>
                        = std::collections::HashMap::new();
                    for (node_id, nickname, group_id) in &peers {
                        if !group_id.starts_with('_') {
                            groups_map.entry(group_id.clone())
                                .or_default()
                                .push((node_id.clone(), nickname.clone()));
                        }
                    }

                    let groups: Vec<DiscoveredGroup> = groups_map.into_iter()
                        .map(|(group_id, members)| DiscoveredGroup { group_id, members })
                        .collect();

                    let peer_count = swarm.connected_peers().await.len();
                    if groups.is_empty() {
                        SearchResult::Empty { peer_count }
                    } else {
                        SearchResult::Found { groups }
                    }
                });

                match crate::swarm_manager::block_on(handle) {
                    Ok(result) => { let _ = tx.send(result); }
                    Err(e) => { let _ = tx.send(SearchResult::Error(format!("{}", e))); }
                }
            }
            Err(e) => {
                log::error!("Failed to init search swarm: {:?}", e);
                let _ = tx.send(SearchResult::Error(format!("{}", e)));
            }
        }
    });

    rx
}

pub fn apply_search_result(model: &mut AppModel, result: SearchResult) {
    match result {
        SearchResult::Found { groups } => {
            model.discovered_groups = groups;
            model.search_error = None;
            model.swarm_state = SwarmState::Connected;
        }
        SearchResult::Empty { peer_count } => {
            model.discovered_groups.clear();
            model.search_error = Some(if peer_count == 0 {
                "未发现任何节点，请确认其他客户端已启动".to_string()
            } else {
                format!("发现 {} 个节点，但没有已创建的群组", peer_count)
            });
            model.swarm_state = SwarmState::Disconnected;
        }
        SearchResult::Error(msg) => {
            model.discovered_groups.clear();
            model.search_error = Some(format!("搜索失败: {}", msg));
            model.swarm_state = SwarmState::Error;
        }
    }
}

pub fn cancel_search(model: &mut AppModel) {
    log::info!("Search cancelled, returning to discovery");

    model.state = AppState::GroupDiscovery;
    model.discovered_groups.clear();
    model.search_error = None;
    model.swarm_state = SwarmState::Disconnected;

    std::thread::spawn(|| {
        crate::swarm_manager::shutdown_swarm();
    });
}

// ── Group Join / Leave ──────────────────────────────────────────────

pub fn create_new_group(model: &mut AppModel) -> String {
    log::info!("Creating new group...");
    let group_id = format!("group_{}", chrono::Utc::now().timestamp());
    model.push_toast(Toast::success(format!("创建群组: {}", group_id)));
    group_id
}

pub fn prepare_join(model: &mut AppModel, group_id: &str) {
    let mut me = AvatarModel::new(
        model.current_user.id.clone(),
        model.current_user.nickname.clone(),
    );
    me.role = Role::Leader;
    me.status = OnlineStatus::Online;

    let lobby = GroupLobbyModel::new(group_id.to_string(), me);

    model.group_lobby = Some(lobby);
    model.state = AppState::GroupLobby;
    model.swarm_state = SwarmState::Connecting;
}

pub fn join_group(
    model: &mut AppModel,
    group_id: String,
    config: &Config,
) -> mpsc::Receiver<JoinResult> {
    log::info!("Joining group: {}", group_id);

    prepare_join(model, &group_id);

    let swarm_path = config.swarm_path(&model.current_user.id).join(&group_id);
    let nickname = model.current_user.nickname.clone();
    let (tx, rx) = mpsc::channel::<JoinResult>();

    std::thread::spawn(move || {
        // Shutdown old swarm (if any) on background thread to avoid blocking UI
        crate::swarm_manager::shutdown_swarm();

        let swarm_config = crate::swarm_manager::SwarmConfig {
            storage_path: swarm_path,
            group_id: group_id.clone(),
        };

        match crate::swarm_manager::get_or_init_swarm(swarm_config) {
            Ok(swarm) => {
                log::info!("Swarm initialized for group: {}", group_id);

                let handle = crate::swarm_manager::spawn({
                    let swarm = swarm.clone();
                    let nickname = nickname.clone();
                    let gid = group_id.clone();
                    async move {
                        if let Err(e) = swarm.announce(&nickname).await {
                            log::error!("Failed to announce: {:?}", e);
                        } else {
                            log::info!("Announced as '{}' in group '{}'", nickname, gid);
                        }

                        // murmur auto-discovery runs in background; just wait for events
                        let mut events = swarm.subscribe();
                        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(8);
                        let mut found = false;
                        loop {
                            tokio::select! {
                                _ = tokio::time::sleep_until(deadline) => break,
                                event = events.recv() => {
                                    match event {
                                        Ok(murmur::SwarmEvent::PeerConnected { .. }) => {
                                            log::info!("Join: peer connected via event");
                                            found = true;
                                            break;
                                        }
                                        Ok(murmur::SwarmEvent::DataSynced) => {
                                            log::info!("Join: data synced via event");
                                            found = true;
                                            break;
                                        }
                                        Ok(murmur::SwarmEvent::PeerDisconnected { .. }) => continue,
                                        Err(_) => break,
                                    }
                                }
                            }
                        }

                        let total = swarm.connected_peers().await.len();
                        if total > 0 || found {
                            log::info!("Connected to {} peer(s)", total);
                            JoinResult::Connected(total.max(1))
                        } else {
                            log::info!("No peers found yet, will continue listening.");
                            JoinResult::NoPeers
                        }
                    }
                });

                match crate::swarm_manager::block_on(handle) {
                    Ok(result) => { let _ = tx.send(result); }
                    Err(e) => { let _ = tx.send(JoinResult::Error(format!("{}", e))); }
                }
            }
            Err(e) => {
                log::error!("Failed to initialize swarm: {:?}", e);
                let _ = tx.send(JoinResult::Error(format!("{}", e)));
            }
        }
    });

    rx
}

pub fn apply_join_result(model: &mut AppModel, result: JoinResult) {
    match result {
        JoinResult::Connected(count) => {
            model.swarm_state = SwarmState::Connected;
            model.push_toast(Toast::success(format!("已连接 {} 个节点", count)));
        }
        JoinResult::NoPeers => {
            model.swarm_state = SwarmState::Connected;
            model.push_toast(Toast::info("群组已创建，等待其他成员加入..."));
        }
        JoinResult::Error(msg) => {
            model.swarm_state = SwarmState::Error;
            model.push_toast(Toast::error(format!("连接失败: {}", msg)));
        }
    }
}

/// Start event-driven member polling — listens for SwarmEvents and refreshes
/// the member list on PeerConnected/Disconnected/DataSynced, with a fallback
/// periodic refresh every 10s.
pub fn start_member_poll() -> mpsc::Receiver<MemberUpdate> {
    let (tx, rx) = mpsc::channel::<MemberUpdate>();

    std::thread::spawn(move || {
        let swarm = match crate::swarm_manager::get_swarm() {
            Some(s) => s,
            None => return,
        };

        let handle = crate::swarm_manager::spawn(async move {
            let mut events = swarm.subscribe();

            // Helper closure to build member list
            let refresh = |swarm: &murmur::Swarm| {
                let swarm = swarm.clone();
                async move {
                    let my_node_id = swarm.node_id().await;
                    let peers = swarm.list_announced_peers().await.unwrap_or_default();
                    let connected = swarm.connected_peers().await;
                    let mut members = Vec::new();
                    for (node_id, nickname, group_id) in peers {
                        if group_id.starts_with('_') || node_id == my_node_id {
                            continue;
                        }
                        let is_connected = connected.iter()
                            .any(|p| p.starts_with(&node_id[..16.min(node_id.len())]));
                        let mut avatar = AvatarModel::new(node_id, nickname);
                        avatar.status = if is_connected { OnlineStatus::Online } else { OnlineStatus::Offline };
                        avatar.role = Role::Member;
                        members.push(avatar);
                    }
                    members
                }
            };

            // Immediate first refresh so we don't miss events that fired before subscribe
            let members = refresh(&swarm).await;
            if tx.send(MemberUpdate { members }).is_err() {
                return;
            }

            loop {
                tokio::select! {
                    event = events.recv() => {
                        match event {
                            Ok(murmur::SwarmEvent::PeerConnected { .. })
                            | Ok(murmur::SwarmEvent::PeerDisconnected { .. })
                            | Ok(murmur::SwarmEvent::DataSynced) => {
                                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {}
                }

                let members = refresh(&swarm).await;
                if tx.send(MemberUpdate { members }).is_err() {
                    break;
                }
            }
        });

        let _ = crate::swarm_manager::block_on(handle);
    });

    rx
}

pub fn apply_member_update(model: &mut AppModel, update: MemberUpdate) {
    if let Some(ref mut lobby) = model.group_lobby {
        lobby.others = update.members;
    }
}

pub fn leave_group(model: &mut AppModel) {
    log::info!("Leaving group");

    model.push_toast(Toast::info("已退出群组"));
    model.group_lobby = None;
    model.shared_file = None;
    model.swarm_state = SwarmState::Disconnected;
    model.state = AppState::GroupDiscovery;

    std::thread::spawn(|| {
        crate::swarm_manager::shutdown_swarm();
    });
}

// ── Collaboration ───────────────────────────────────────────────────

pub fn start_collaboration(model: &mut AppModel, config: &Config) {
    log::info!("Starting collaboration");

    let group_id = match model.group_lobby.as_ref() {
        Some(lobby) => lobby.group_id.clone(),
        None => {
            model.push_toast(Toast::error("无法开始协作：未加入群组"));
            return;
        }
    };

    let shared_file_path = config.shared_file_path.clone();

    if !shared_file_path.exists() {
        if let Err(e) = std::fs::write(&shared_file_path, format!(
            "# 群组协作文件: {}\n\n欢迎来到协作空间！\n\n", group_id
        )) {
            log::error!("Failed to create shared file: {:?}", e);
            model.push_toast(Toast::error("创建共享文件失败"));
            return;
        }
    }

    model.push_toast(Toast::success("开始协作！共享文件: chat.ctx"));

    let file_path = shared_file_path.clone();
    let group_id_for_thread = group_id.clone();

    std::thread::spawn(move || {
        match crate::swarm_manager::get_swarm() {
            Some(swarm) => {
                log::info!("Using global Swarm for SharedFile");
                crate::swarm_manager::block_on(async move {
                    match SharedFile::new(swarm, "chat_ctx".to_string(), file_path.clone()).await {
                        Ok(shared_file) => {
                            log::info!("SharedFile initialized for group: {}", group_id_for_thread);
                            let shared_file = Arc::new(shared_file);
                            if let Err(e) = shared_file.start_watching().await {
                                log::error!("Failed to start file watching: {:?}", e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to initialize SharedFile: {:?}", e);
                        }
                    }
                });
            }
            None => {
                log::error!("No global Swarm instance found!");
            }
        }
    });

    log::info!("Collaboration started for group: {}", group_id);
}
