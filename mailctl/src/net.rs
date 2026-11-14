//! 传输层：明文 TCP / rustls TLS / STARTTLS 升级 + 行缓冲读写。

use crate::config::Security;
use anyhow::{anyhow, bail, Context, Result};
use rustls::{ClientConnection, RootCertStore, StreamOwned};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

pub fn tls_config() -> Arc<rustls::ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

fn server_name(host: &str) -> Result<rustls::pki_types::ServerName<'static>> {
    rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow!("非法 TLS 主机名：{host}"))
}

pub enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

impl Stream {
    /// 把明文连接升级成 TLS（STARTTLS 用）。
    pub fn into_tls(self, host: &str) -> Result<Stream> {
        let tcp = match self {
            Stream::Plain(s) => s,
            Stream::Tls(_) => bail!("连接已是 TLS，不能再次升级"),
        };
        let name = server_name(host)?;
        let conn = ClientConnection::new(tls_config(), name)
            .map_err(|e| anyhow!("TLS 握手初始化失败：{e}"))?;
        Ok(Stream::Tls(Box::new(StreamOwned::new(conn, tcp))))
    }
}

pub fn connect(host: &str, port: u16, sec: Security) -> Result<Stream> {
    let tcp = TcpStream::connect((host, port))
        .with_context(|| format!("连不上 {host}:{port}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(60)))?;
    let _ = tcp.set_nodelay(true);
    match sec {
        Security::Tls => {
            let name = server_name(host)?;
            let conn = ClientConnection::new(tls_config(), name)
                .map_err(|e| anyhow!("TLS 握手初始化失败：{e}"))?;
            Ok(Stream::Tls(Box::new(StreamOwned::new(conn, tcp))))
        }
        Security::StartTls | Security::Plain => Ok(Stream::Plain(tcp)),
    }
}

/// 带缓冲的连接：支持按行读、按长度读字面量（IMAP literal）。
pub struct Conn {
    inner: Option<BufReader<Stream>>,
}

impl Conn {
    pub fn new(stream: Stream) -> Self {
        Conn { inner: Some(BufReader::with_capacity(16 * 1024, stream)) }
    }

    fn buf(&mut self) -> &mut BufReader<Stream> {
        self.inner.as_mut().expect("连接已失效")
    }

    pub fn read_line(&mut self) -> Result<String> {
        let mut raw = Vec::new();
        let n = self
            .buf()
            .read_until(b'\n', &mut raw)
            .context("读超时或连接中断")?;
        if n == 0 {
            bail!("对端关闭连接");
        }
        while raw.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            raw.pop();
        }
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    pub fn read_exact_len(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        self.buf()
            .read_exact(&mut buf)
            .with_context(|| format!("读 {n} 字节字面量失败"))?;
        Ok(buf)
    }

    pub fn write_line(&mut self, line: &str) -> Result<()> {
        self.buf()
            .get_mut()
            .write_all(format!("{line}\r\n").as_bytes())
            .context("写失败")?;
        self.buf().get_mut().flush().context("flush 失败")?;
        Ok(())
    }

    pub fn write_raw(&mut self, data: &[u8]) -> Result<()> {
        self.buf().get_mut().write_all(data).context("写失败")?;
        self.buf().get_mut().flush().context("flush 失败")?;
        Ok(())
    }

    /// STARTTLS：把内层明文流取出、升级为 TLS、重新包缓冲。
    pub fn upgrade_tls(&mut self, host: &str) -> Result<()> {
        if !self.buf().buffer().is_empty() {
            bail!("STARTTLS 前缓冲区非空，拒绝升级");
        }
        let reader = self.inner.take().expect("连接已失效");
        let stream = reader.into_inner();
        let upgraded = stream.into_tls(host)?;
        self.inner = Some(BufReader::with_capacity(16 * 1024, upgraded));
        Ok(())
    }
}
