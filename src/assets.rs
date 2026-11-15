//! 前端产物（web/dist）打包进二进制。
//!
//! Vite 构建出的 dist 目录在编译期被 include_dir 读进只读静态段，
//! 运行时不需要磁盘上存在 web/dist —— 单文件分发即可打开 Web 控制台。
//! 开发前端时仍可直接 pnpm dev，那时后端只负责 API。
//!
//! include_dir 的宏只接受字面量路径，这里用 $CARGO_MANIFEST_DIR 展开，
//! 保证换机器 / CI 里都能找到同仓库的 web/dist。

use include_dir::{include_dir, Dir, File};

static DIST: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

/// 相对 dist 根取一个文件；拒绝任何 .. 穿越。
///
/// 找不到且路径「看起来像前端路由」（没有扩展名）时回落到 index.html，
/// 让 SPA 的客户端路由也能直接刷新。
pub fn get(path: &str) -> Option<&'static File<'static>> {
    let clean = path.trim_start_matches('/');
    if clean.is_empty() {
        return DIST.get_file("index.html");
    }
    if clean.split('/').any(|c| c == ".." || c.is_empty()) {
        return None;
    }
    if let Some(f) = DIST.get_file(clean) {
        return Some(f);
    }
    let looks_like_route = std::path::Path::new(clean).extension().is_none();
    if looks_like_route {
        return DIST.get_file("index.html");
    }
    None
}

pub fn index_html() -> Option<&'static [u8]> {
    DIST.get_file("index.html").map(|f| f.contents())
}

/// 前端产物指纹：对嵌入 dist 的路径与内容做稳定哈希，旧页面据此自动刷新。
pub fn build_id() -> Option<String> {
    if !embedded() {
        return None;
    }
    let mut files: Vec<(String, &[u8])> = Vec::new();
    collect("", &DIST, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h: u64 = 0xcbf29ce484222325;
    for (path, bytes) in files {
        for b in path
            .as_bytes()
            .iter()
            .copied()
            .chain([0u8])
            .chain(bytes.iter().copied())
        {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    Some(format!("{h:016x}"))
}

/// 前端是否编译进二进制（决定 /api/health 里的 webMode）
pub fn embedded() -> bool {
    index_html().is_some()
}

fn collect<'a>(prefix: &str, dir: &'a Dir<'static>, out: &mut Vec<(String, &'a [u8])>) {
    for f in dir.files() {
        let name = f.path().to_string_lossy();
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        out.push((path, f.contents()));
    }
    for d in dir.dirs() {
        let name = d
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        collect(&path, d, out);
    }
}
