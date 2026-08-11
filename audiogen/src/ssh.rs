//! Pure-Rust SSH client (russh) — runs shell commands on the rustyme VPS.
//!
//! Used to drive `redis-cli` on the remote host (VPS redis port is firewalled,
//! must SSH in to LPUSH/BRPOP). Async via tokio.

use anyhow::{anyhow, Context, Result};
use futures::FutureExt;
use russh::client::{self, AuthResult, Handler};
use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg};
use russh::ChannelMsg;
use std::path::Path;
use std::pin::Pin;
use std::future::Future;
use std::sync::Arc;

pub struct SshClient {
    session: client::Handle<ClientHandler>,
}

struct ClientHandler;

impl Handler for ClientHandler {
    type Error = russh::Error;

    #[allow(refining_impl_trait)]
    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Pin<Box<dyn Future<Output = Result<bool, Self::Error>> + Send + '_>> {
        async move { Ok::<bool, russh::Error>(true) }.boxed()
    }
}

/// Convenience: parse `user@host:port` (or `user@host`) and connect.
pub async fn connect_host(spec: &str, key_path: Option<&Path>) -> Result<SshClient> {
    let (user_host, port) = match spec.rsplit_once(':') {
        Some((uh, p)) => (uh, p.parse::<u16>().unwrap_or(22)),
        None => (spec, 22),
    };
    let (user, host) = user_host.split_once('@')
        .ok_or_else(|| anyhow!("vps spec must be `user@host[:port]`, got {spec}"))?;
    connect(host, port, user, key_path).await
}

/// Connect via SSH key auth. Reads private key from `key_path`.
/// If `key_path` is None, tries ssh-agent.
pub async fn connect(host: &str, port: u16, user: &str, key_path: Option<&Path>) -> Result<SshClient> {
    let config = Arc::new(client::Config::default());
    let handler = ClientHandler;
    let mut session = client::connect(config, (host, port), handler).await
        .with_context(|| format!("ssh connect {host}:{port}"))?;

    let authenticated = if let Some(kp) = key_path {
        let key = load_secret_key(kp, None)
            .with_context(|| format!("load ssh key {}", kp.display()))?;
        let hash_alg = match session.best_supported_rsa_hash().await? {
            Some(Some(alg)) => alg,
            _ => HashAlg::Sha256,
        };
        let pwkh = PrivateKeyWithHashAlg::new(Arc::new(key), Some(hash_alg));
        let res = session.authenticate_publickey(user, pwkh).await?;
        matches!(res, AuthResult::Success)
    } else {
        // ssh-agent fallback
        use russh::keys::agent::client::AgentClient;
        let mut agent = AgentClient::connect_env().await
            .context("connect ssh-agent (SSH_AUTH_SOCK)")?;
        let identities = agent.request_identities().await?;
        let mut ok = false;
        for identity in identities {
            // authenticate_publickey_with takes PublicKey + signer (the AgentClient).
            let pk = identity.public_key().into_owned();
            let res = session
                .authenticate_publickey_with(user, pk, None, &mut agent)
                .await
                .map_err(|_| anyhow!("ssh-agent signature error"))?;
            if matches!(res, AuthResult::Success) {
                ok = true;
                break;
            }
        }
        ok
    };
    if !authenticated {
        anyhow::bail!("ssh auth failed for {user}@{host}");
    }
    Ok(SshClient { session })
}

impl SshClient {
    async fn run_channel(&mut self, cmd: &str, stdin_data: Option<&[u8]>) -> Result<(String, String, i32)> {
        let mut ch = self.session.channel_open_session().await
            .context("open channel")?;
        ch.exec(true, cmd).await.context("exec remote cmd")?;
        if let Some(data) = stdin_data {
            ch.data(data).await.context("stdin write")?;
            ch.eof().await.context("stdin eof")?;
        }
        let mut out = String::new();
        let mut err = String::new();
        let mut code = 0i32;
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    out.push_str(&String::from_utf8_lossy(&data));
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    err.push_str(&String::from_utf8_lossy(&data));
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    code = exit_status as i32;
                }
                _ => {}
            }
        }
        Ok((out, err, code))
    }

    /// Run a shell command, return (stdout, stderr, exit_code).
    pub async fn exec(&mut self, cmd: &str) -> Result<(String, String, i32)> {
        self.run_channel(cmd, None).await
    }

    /// Run a command, piping `stdin_data` (LPUSH via `redis-cli -x`).
    pub async fn exec_stdin(&mut self, cmd: &str, stdin_data: &[u8]) -> Result<(String, String, i32)> {
        self.run_channel(cmd, Some(stdin_data)).await
    }
}