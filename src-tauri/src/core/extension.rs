//! 浏览器扩展的一键安装。
//!
//! 浏览器**不允许**程序替用户装扩展（安全限制），所以"安装"实际是三步里能自动的两步：
//!
//! 1. 把扩展源码写到 `%APPDATA%\dev.lunar.lemma\extension\`
//! 2. 写 Native Messaging 宿主清单 + 注册表键（Chrome / Edge）与 Firefox 清单
//! 3. 剩下最后一步必须用户自己做：`chrome://extensions` 打开开发者模式 → 加载已解压的扩展
//!
//! 扩展 ID 由 `manifest.json` 里的 `key` 固定，所以宿主清单可以写死白名单，
//! 用户不必回填 ID。

use std::path::{Path, PathBuf};

/// 扩展源码（编译期嵌入，避免安装包资源路径的坑）。
const MANIFEST: &str = include_str!("../../../extension/manifest.json");
const BACKGROUND: &str = include_str!("../../../extension/background.js");
const CONTENT: &str = include_str!("../../../extension/content.js");
const ICON_16: &[u8] = include_bytes!("../../../extension/icons/16.png");
const ICON_32: &[u8] = include_bytes!("../../../extension/icons/32.png");
const ICON_48: &[u8] = include_bytes!("../../../extension/icons/48.png");
const ICON_128: &[u8] = include_bytes!("../../../extension/icons/128.png");

/// 扩展 ID：由 `extension/manifest.json` 的 `key` 固定，宿主白名单写死它。
pub const EXTENSION_ID: &str = "kchhodaiilomkholhbpajlgkblnikfpb";

/// Firefox 用 gecko id 而不是 chrome extension id。
const FIREFOX_EXTENSION_ID: &str = "lemma-writeback@lunar.dev";

/// 宿主名，两处清单与扩展里必须一致。
const HOST_NAME: &str = "dev.lunar.lemma";

pub struct InstallReport {
    pub extension_dir: String,
    pub host_manifest: String,
    pub firefox_manifest: String,
    pub registry: bool,
    pub error: Option<String>,
}

/// 扩展目录（也用于判断"装过没装过"）。
pub fn extension_dir() -> Option<PathBuf> {
    crate::core::paths::app_data_dir().map(|dir| dir.join("extension"))
}

/// 装过没有：扩展文件与宿主清单都在。
pub fn is_installed() -> bool {
    let Some(dir) = extension_dir() else {
        return false;
    };
    dir.join("manifest.json").exists() && host_manifest_path().is_some_and(|path| path.exists())
}

fn host_dir() -> Option<PathBuf> {
    crate::core::paths::app_data_dir().map(|dir| dir.join("native-host"))
}

/// Chrome/Edge 读的那份宿主清单。
pub fn host_manifest_path() -> Option<PathBuf> {
    host_dir().map(|dir| dir.join(format!("{HOST_NAME}.json")))
}

fn firefox_manifest_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| {
        PathBuf::from(base)
            .join("Mozilla")
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"))
    })
}

/// 执行安装：写扩展文件、写两份宿主清单、写注册表。
pub fn install() -> InstallReport {
    let mut report = InstallReport {
        extension_dir: String::new(),
        host_manifest: String::new(),
        firefox_manifest: String::new(),
        registry: false,
        error: None,
    };

    let Some(dir) = extension_dir() else {
        report.error = Some("拿不到应用数据目录".to_string());
        return report;
    };
    if let Err(error) = write_extension(&dir) {
        report.error = Some(error);
        return report;
    }
    report.extension_dir = dir.display().to_string();

    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    // Chrome / Edge 共用同一份清单文件，只是注册表位置不同。
    // 注意：必须用 serde 序列化，不能字符串替换——Windows 路径里的 `\C` 在 JSON 里是
    // 非法转义，Chrome 会因此报 "native messaging host not found"（实测踩到）。
    let chrome_manifest = serde_json::to_string_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "Lemma 桌面应用的 Native Messaging 宿主",
        "path": exe,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    }))
    .unwrap_or_default();
    let Some(host_path) = host_manifest_path() else {
        report.error = Some("拿不到宿主清单目录".to_string());
        return report;
    };
    if let Err(error) = write_file(&host_path, &chrome_manifest) {
        report.error = Some(error);
        return report;
    }
    report.host_manifest = host_path.display().to_string();
    report.registry = register_chrome(&host_path);

    // Firefox 自己读清单，不需要注册表
    if let Some(firefox_path) = firefox_manifest_path() {
        let firefox_manifest = serde_json::to_string_pretty(&serde_json::json!({
            "name": HOST_NAME,
            "description": "Lemma 桌面应用的 Native Messaging 宿主",
            "path": exe,
            "type": "stdio",
            "allowed_extensions": [FIREFOX_EXTENSION_ID],
        }))
        .unwrap_or_default();
        match write_file(&firefox_path, &firefox_manifest) {
            Ok(()) => report.firefox_manifest = firefox_path.display().to_string(),
            Err(error) => {
                crate::core::logs::line(format!("extension: Firefox 清单写入失败 {error}"));
            }
        }
    }

    crate::core::logs::line(format!(
        "extension installed dir={} registry={} chrome={} firefox={}",
        report.extension_dir, report.registry, report.host_manifest, report.firefox_manifest
    ));
    report
}

fn write_extension(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建扩展目录失败：{e}"))?;
    for (name, content) in [
        ("manifest.json", MANIFEST),
        ("background.js", BACKGROUND),
        ("content.js", CONTENT),
    ] {
        write_file(&dir.join(name), content)?;
    }
    let icons = dir.join("icons");
    std::fs::create_dir_all(&icons).map_err(|e| format!("建图标目录失败：{e}"))?;
    for (name, bytes) in [
        ("16.png", ICON_16),
        ("32.png", ICON_32),
        ("48.png", ICON_48),
        ("128.png", ICON_128),
    ] {
        std::fs::write(icons.join(name), bytes).map_err(|e| format!("写图标失败：{e}"))?;
    }
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
    }
    std::fs::write(path, content).map_err(|e| format!("写 {} 失败：{e}", path.display()))
}

fn register_chrome(manifest: &Path) -> bool {
    crate::platform::register_native_host(HOST_NAME, &manifest.display().to_string())
}
/// 扩展的 GitHub 下载地址（Release 资产）。
pub const DOWNLOAD_URL: &str =
    "https://github.com/veyastra2362/Lemma/releases/download/extension-v1/lemma-extension.zip";

/// 把扩展的三个文件打成一个 zip（用户下载后解压即可加载）。
///
/// 为什么要打包：Chrome 只能"加载已解压的扩展程序"，让用户从 GitHub 一个个下载
/// 三个文件太蠢；一个 zip 解压即用。
pub fn package(out: &std::path::Path) -> Result<u64, String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in [
            ("manifest.json", MANIFEST),
            ("background.js", BACKGROUND),
            ("content.js", CONTENT),
        ] {
            zip.start_file(name, options)
                .map_err(|e| format!("写 zip 条目失败：{e}"))?;
            zip.write_all(content.as_bytes())
                .map_err(|e| format!("写 zip 内容失败：{e}"))?;
        }
        for (name, bytes) in [
            ("icons/16.png", ICON_16),
            ("icons/32.png", ICON_32),
            ("icons/48.png", ICON_48),
            ("icons/128.png", ICON_128),
        ] {
            zip.start_file(name, options)
                .map_err(|e| format!("写 zip 条目失败：{e}"))?;
            zip.write_all(bytes)
                .map_err(|e| format!("写图标失败：{e}"))?;
        }
        zip.finish().map_err(|e| format!("收尾失败：{e}"))?;
    }
    let bytes = buffer.into_inner();
    std::fs::write(out, &bytes).map_err(|e| format!("写文件失败：{e}"))?;
    Ok(bytes.len() as u64)
}
