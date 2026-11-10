//! 目录约定：配置 / 数据 / 默认输出目录
//!
//! 全部可用环境变量覆盖，便于测试与多实例：
//! - LYCODL_CONFIG_DIR：配置目录（默认 ~/.config/lycodl）
//! - LYCODL_DATA_DIR：数据目录（默认 ~/.local/share/lycodl，托管内核二进制）
//! - LYCODL_OUTPUT_DIR：默认下载目录
//! - LYCODL_YTDLP：显式指定 yt-dlp 可执行文件

use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn project() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "lycodl")
}

pub fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("LYCODL_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    project()
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(|| home_dir().join(".config/lycodl"))
}

pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("LYCODL_DATA_DIR") {
        return PathBuf::from(p);
    }
    project()
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| home_dir().join(".local/share/lycodl"))
}

/// 默认下载目录：XDG 下载目录下的 lycodl/（无 XDG 时退到 ~/Downloads/lycodl）
pub fn default_output_dir() -> PathBuf {
    if let Some(ud) = directories::UserDirs::new() {
        if let Some(dl) = ud.download_dir() {
            return dl.join("lycodl");
        }
    }
    home_dir().join("Downloads/lycodl")
}

/// 由 lycodl 自己管理的 yt-dlp 二进制（热更新写这里，优先于系统那份）
pub fn managed_ytdlp() -> PathBuf {
    data_dir().join("bin").join("yt-dlp")
}
