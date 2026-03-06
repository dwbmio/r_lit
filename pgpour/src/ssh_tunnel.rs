use std::sync::Arc;

use russh::client::{self, AuthResult};
use russh::keys::{self, PrivateKeyWithHashAlg, PublicKey};
use tokio::io::copy_bidirectional;
use tokio::net::TcpListener;
use tracing::{debug, error, info};

struct TunnelHandler;

impl client::Handler for TunnelHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key_path: Option<String>,
    pub private_key_passphrase: Option<String>,
    /// PG host as seen from the SSH server
    pub remote_host: String,
    /// PG port as seen from the SSH server
    pub remote_port: u16,
}

pub struct SshTunnel {
    pub local_port: u16,
}

impl SshTunnel {
    pub async fn start(cfg: SshTunnelConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let ssh_config = Arc::new(client::Config::default());
        let mut handle =
            client::connect(ssh_config, (&*cfg.host, cfg.port), TunnelHandler).await?;

        let auth_result = if let Some(ref key_path) = cfg.private_key_path {
            let key =
                keys::load_secret_key(key_path, cfg.private_key_passphrase.as_deref())?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await?
                .unwrap_or(None);
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            handle.authenticate_publickey(&cfg.username, key).await?
        } else if let Some(ref password) = cfg.password {
            handle
                .authenticate_password(&cfg.username, password)
                .await?
        } else {
            return Err("SSH tunnel requires either private_key_path or password".into());
        };

        if auth_result != AuthResult::Success {
            return Err(format!("SSH authentication failed: {auth_result:?}").into());
        }

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_port = listener.local_addr()?.port();
        let remote_host = cfg.remote_host;
        let remote_port = cfg.remote_port;

        info!(
            local_port,
            ssh = format!("{}@{}:{}", cfg.username, cfg.host, cfg.port),
            remote = format!("{remote_host}:{remote_port}"),
            "SSH tunnel listening"
        );

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((mut tcp, peer)) => {
                        debug!(%peer, "tunnel: accepted");
                        match handle
                            .channel_open_direct_tcpip(
                                remote_host.clone(),
                                remote_port as u32,
                                "127.0.0.1",
                                0,
                            )
                            .await
                        {
                            Ok(channel) => {
                                tokio::spawn(async move {
                                    let mut stream = channel.into_stream();
                                    if let Err(e) =
                                        copy_bidirectional(&mut tcp, &mut stream).await
                                    {
                                        debug!(%peer, %e, "tunnel pipe closed");
                                    }
                                });
                            }
                            Err(e) => error!(%peer, %e, "tunnel: SSH channel open failed"),
                        }
                    }
                    Err(e) => {
                        error!(%e, "tunnel: accept failed");
                        break;
                    }
                }
            }
        });

        Ok(Self { local_port })
    }
}
