//! 本地 HTTPS 解密 CA：
//! - 首次使用时生成一张本地根证书（ECDSA P-256），保存在应用数据目录
//! - 为每个目标域名动态签发叶子证书并缓存
//! - 支持一键安装到 Windows 当前用户 Root 证书存储
//!
//! 根证书私钥只保存在本机文件，不会通过 IPC 暴露给前端。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;

use crate::models::HttpsCaInfo;

/// 会显示在浏览器证书详情里的根证书名称
const CA_COMMON_NAME: &str = "FreeCharles Local CA";
const CA_ORGANIZATION: &str = "FreeCharles";
/// 叶子证书有效期（天），控制在 365 天内以兼容 iOS / Safari 的限制
const LEAF_VALID_DAYS: i64 = 365;
/// 叶子证书缓存上限，避免长时间运行后无限增长
const MAX_CACHED_SERVERS: usize = 500;

struct CaState {
    ca_cert: Certificate,
    ca_key: KeyPair,
    ca_pem: String,
    leaf_key: KeyPair,
}

pub struct HttpsCa {
    dir: PathBuf,
    cert_path: PathBuf,
    key_path: PathBuf,
    state: Mutex<Option<CaState>>,
    servers: Mutex<HashMap<String, Arc<ServerConfig>>>,
    last_error: Mutex<String>,
}

impl HttpsCa {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            cert_path: dir.join("freecharles-ca.crt"),
            key_path: dir.join("freecharles-ca.key"),
            dir,
            state: Mutex::new(None),
            servers: Mutex::new(HashMap::new()),
            last_error: Mutex::new(String::new()),
        }
    }

    pub fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    /// 根证书文件是否已生成（只检查文件，不触发加载）
    pub fn generated(&self) -> bool {
        self.cert_path.exists() && self.key_path.exists()
    }

    /// 启动时预加载已有根证书，避免首个 HTTPS 连接时再做解析
    pub fn warmup(&self) {
        if self.generated() {
            let _ = self.ensure_loaded();
        }
    }

    /// 是否可以去执行 TLS 中间人解密
    pub fn ready(&self) -> bool {
        self.generated()
    }

    pub fn info(&self) -> HttpsCaInfo {
        let generated = self.generated();
        let error = self.take_error_snapshot();
        HttpsCaInfo {
            supported: true,
            installable: cfg!(windows),
            generated,
            cert_path: if generated {
                Some(self.cert_path.display().to_string())
            } else {
                None
            },
            cert_pem: if generated {
                std::fs::read_to_string(&self.cert_path).ok()
            } else {
                None
            },
            platform: std::env::consts::OS.to_string(),
            message: Some(describe_message(generated)),
            error: if error.is_empty() { None } else { Some(error) },
        }
    }

    /// 加载或生成根证书，返回最新状态
    pub fn ensure(&self) -> HttpsCaInfo {
        let _ = self.ensure_loaded();
        self.info()
    }

    /// 安装根证书到系统信任存储（仅 Windows 支持一键安装）
    pub fn install(&self) -> HttpsCaInfo {
        let info = self.ensure();
        if !info.generated {
            return info;
        }

        if !cfg!(windows) {
            self.set_error(String::new());
            return HttpsCaInfo {
                message: Some(
                    "当前平台需要手动安装证书。已生成证书文件，请将它导入浏览器或系统信任存储。"
                        .to_string(),
                ),
                ..info
            };
        }

        match install_into_trust_store(&self.cert_path) {
            Ok(()) => {
                self.set_error(String::new());
                HttpsCaInfo {
                    message: Some(
                        "根证书已安装到当前用户 Root 证书存储；如果浏览器仍报错，请重启浏览器。"
                            .to_string(),
                    ),
                    ..self.info()
                }
            }
            Err(message) => {
                self.set_error(message);
                self.info()
            }
        }
    }

    /// 取得指定目标域名的 TLS 服务端配置（内部会签发并缓存证书）
    pub fn server_config(&self, host: &str) -> Result<Arc<ServerConfig>, String> {
        let key = normalize_host(host);
        if let Some(config) = self
            .servers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(config);
        }

        self.ensure_loaded()?;
        let (cert_der, key_der) = {
            let guard = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = guard
                .as_ref()
                .ok_or_else(|| "本地 HTTPS 根证书尚未准备完成".to_string())?;
            sign_leaf(state, &key)?
        };

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert_der)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
            )
            .map_err(|error| format!("HTTPS 解密证书配置失败：{error}"))?;
        // 解密后的流量按 HTTP/1.1 交给本地处理逻辑
        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        let config = Arc::new(config);
        let mut servers = self
            .servers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if servers.len() > MAX_CACHED_SERVERS {
            servers.clear();
        }
        servers.insert(key, config.clone());
        Ok(config)
    }

    fn ensure_loaded(&self) -> Result<(), String> {
        if self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
        {
            return Ok(());
        }

        let result = self.load_or_generate();
        match &result {
            Ok(()) => self.set_error(String::new()),
            Err(message) => self.set_error(message.clone()),
        }
        result
    }

    fn load_or_generate(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|error| format!("创建根证书目录失败：{error}"))?;

        if self.generated() {
            match self.load() {
                Ok(state) => {
                    self.set_state(state);
                    return Ok(());
                }
                Err(message) => {
                    // 文件损坏时重新生成；旧证书的信任需要用户重新安装
                    self.set_error(format!("已有根证书读取失败，将重新生成：{message}"));
                }
            }
        }

        let state = self.generate()?;
        self.set_state(state);
        Ok(())
    }

    fn load(&self) -> Result<CaState, String> {
        let ca_pem = std::fs::read_to_string(&self.cert_path)
            .map_err(|error| format!("读取根证书失败：{error}"))?;
        let key_pem = std::fs::read_to_string(&self.key_path)
            .map_err(|error| format!("读取根证书私钥失败：{error}"))?;
        let params = CertificateParams::from_ca_cert_pem(&ca_pem)
            .map_err(|error| format!("根证书解析失败：{error}"))?;
        let ca_key =
            KeyPair::from_pem(&key_pem).map_err(|error| format!("根证书私钥解析失败：{error}"))?;
        // 用同一把私钥重建签发者对象，subject / 公钥与磁盘文件一致
        let ca_cert = params
            .self_signed(&ca_key)
            .map_err(|error| format!("根证书重建失败：{error}"))?;
        let leaf_key = KeyPair::generate().map_err(|error| format!("生成叶子密钥失败：{error}"))?;

        Ok(CaState {
            ca_cert,
            ca_key,
            ca_pem,
            leaf_key,
        })
    }

    fn generate(&self) -> Result<CaState, String> {
        let ca_key = KeyPair::generate().map_err(|error| format!("生成根证书私钥失败：{error}"))?;
        let mut params =
            CertificateParams::new(Vec::<String>::new()).map_err(|error| error.to_string())?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, CA_COMMON_NAME);
        name.push(DnType::OrganizationName, CA_ORGANIZATION);
        name.push(DnType::OrganizationalUnitName, "Local HTTPS Debugging");
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        let ca_cert = params
            .self_signed(&ca_key)
            .map_err(|error| format!("生成根证书失败：{error}"))?;
        let ca_pem = ca_cert.pem();
        std::fs::write(&self.cert_path, &ca_pem)
            .map_err(|error| format!("写入根证书失败：{error}"))?;
        std::fs::write(&self.key_path, ca_key.serialize_pem())
            .map_err(|error| format!("写入根证书私钥失败：{error}"))?;
        let leaf_key = KeyPair::generate().map_err(|error| format!("生成叶子密钥失败：{error}"))?;

        Ok(CaState {
            ca_cert,
            ca_key,
            ca_pem,
            leaf_key,
        })
    }

    fn set_state(&self, state: CaState) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(state);
        // 根证书变化后旧缓存全部失效
        self.servers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn set_error(&self, message: String) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = message;
    }

    fn take_error_snapshot(&self) -> String {
        self.last_error
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// 供其他地方读取根证书 PEM（例如导出或展示）
    pub fn ca_pem(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|state| state.ca_pem.clone()))
    }
}

fn describe_message(generated: bool) -> String {
    if !generated {
        return "尚未生成 HTTPS 解密根证书".to_string();
    }
    if cfg!(windows) {
        "可安装到 Windows 当前用户 Root 证书存储".to_string()
    } else {
        "根证书已生成，请手动安装到浏览器或系统信任存储".to_string()
    }
}

/// 为目标域名签发叶子证书，返回 (证书 DER, 私钥 PKCS#8 DER)
fn sign_leaf(state: &CaState, host: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut params =
        CertificateParams::new(vec![host.to_string()]).map_err(|error| error.to_string())?;
    let mut name = DistinguishedName::new();
    // CN 长度上限 64，超长时截断，SAN 里保留完整域名
    name.push(
        DnType::CommonName,
        host.chars().take(64).collect::<String>(),
    );
    params.distinguished_name = name;
    params.is_ca = IsCa::NoCa;
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(LEAF_VALID_DAYS);

    let cert = params
        .signed_by(&state.leaf_key, &state.ca_cert, &state.ca_key)
        .map_err(|error| format!("签发 {host} 的证书失败：{error}"))?;
    Ok((cert.der().to_vec(), state.leaf_key.serialize_der()))
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn install_into_trust_store(cert_path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    /// CREATE_NO_WINDOW：避免弹出控制台窗口
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = std::process::Command::new("certutil")
        .args(["-user", "-addstore", "-f", "Root"])
        .arg(cert_path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("调用 certutil 失败：{error}"))?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(format!(
        "安装根证书失败：{}",
        if detail.is_empty() {
            "certutil 返回非零退出码".to_string()
        } else {
            detail
        }
    ))
}

#[cfg(not(windows))]
fn install_into_trust_store(_cert_path: &Path) -> Result<(), String> {
    Err("当前平台不支持一键安装，请手动导入根证书".to_string())
}
