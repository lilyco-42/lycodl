//! 站点目录与解析策略记忆：全平台支持的「地图」。
//!
//! yt-dlp 现有一千多个提取器，但人只关心几十个常用站。这里给它们配上
//! 中文名、分类、域名、状态和「为什么会失败」的提示，再记住每个站最后
//! 生效的解析策略 —— 下次直接走那条路，不必把坑重新踩一遍。

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteInfo {
    pub key: String,
    pub name: String,
    pub category: String,
    pub domains: Vec<String>,
    /// ready / needs-cookies / needs-login / experimental
    pub status: String,
    pub tips: Vec<String>,
    pub sample_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitesResponse {
    pub curated: Vec<SiteInfo>,
    /// 最近一次解析命中的站点名（没有则为 null，前端据此决定是否显示）
    pub matched: Option<String>,
    pub strategies: BTreeMap<String, String>,
}

struct Def {
    key: &'static str,
    name: &'static str,
    category: &'static str,
    domains: &'static [&'static str],
    status: &'static str,
    tips: &'static [&'static str],
    sample: &'static str,
    /// 同一域名下需要靠路径区分的站（如 bilibili 的番剧）
    path_hint: Option<&'static str>,
}

const fn d(
    key: &'static str,
    name: &'static str,
    category: &'static str,
    domains: &'static [&'static str],
    status: &'static str,
    tips: &'static [&'static str],
    sample: &'static str,
) -> Def {
    Def {
        key,
        name,
        category,
        domains,
        status,
        tips,
        sample,
        path_hint: None,
    }
}

// 顺序有意义：带 path_hint 的站要排在它的主站之前。
const SITES: &[Def] = &[
    Def {
        key: "bilibili-bangumi",
        name: "哔哩哔哩番剧",
        category: "anime",
        domains: &["bilibili.com"],
        status: "ready",
        tips: &["番剧本身免费可直接下；大会员专享需要登录状态下的 Cookie"],
        sample: "https://www.bilibili.com/bangumi/play/ep1231564",
        path_hint: Some("/bangumi/"),
    },
    d(
        "youtube",
        "YouTube",
        "video",
        &["youtube.com", "youtu.be"],
        "needs-cookies",
        &[
            "2026 年起 web 端只下发 SABR 流，未登录会要求「确认你不是机器人」，必须选浏览器 Cookie",
            "解签名需要 node / deno / bun 之一，缺了只会报 No video formats found",
            "个别视频上游本身只给 storyboard（预览图），换一个视频试试",
        ],
        "https://www.youtube.com/watch?v=jNQXAC9IVRw",
    ),
    d(
        "bilibili",
        "哔哩哔哩",
        "video",
        &["bilibili.com", "b23.tv"],
        "ready",
        &["普通投稿无需 Cookie；高码率（1080P+ / 4K）需要登录 Cookie"],
        "https://www.bilibili.com/video/BV1GJ411x7h7",
    ),
    d(
        "douyin",
        "抖音",
        "social",
        &["douyin.com", "iesdouyin.com"],
        "needs-cookies",
        &["必须带本机浏览器 Cookie（不必登录），否则报 Fresh cookies are needed"],
        "https://www.douyin.com/video/7153055087843507463",
    ),
    d(
        "tiktok",
        "TikTok",
        "social",
        &["tiktok.com"],
        "ready",
        &["公开视频可直接下；私密/地区限制内容需要 Cookie"],
        "https://www.tiktok.com/@scout2015/video/6718335390845095173",
    ),
    d(
        "xiaohongshu",
        "小红书",
        "social",
        &["xiaohongshu.com", "xhslink.com"],
        "needs-cookies",
        &["不带 Cookie 常报 No video formats found；需要登录过的浏览器"],
        "https://www.xiaohongshu.com/explore/6411cf99000000001300b6d9",
    ),
    d(
        "weibo",
        "微博",
        "social",
        &["weibo.com", "weibo.cn"],
        "ready",
        &["单条微博的 t.cn 短链要展开成完整正文地址"],
        "https://weibo.com/7827771738/N4xlMvjhI",
    ),
    d(
        "instagram",
        "Instagram",
        "social",
        &["instagram.com"],
        "needs-cookies",
        &["公开 reel 可直接下；帖子/私密内容需要登录 Cookie"],
        "https://www.instagram.com/reel/Chunk8-jurw/",
    ),
    d(
        "x",
        "X（Twitter）",
        "social",
        &["x.com", "twitter.com"],
        "needs-cookies",
        &["现在基本都要登录 Cookie；纯文字推文没有视频可下"],
        "https://x.com/NASA/status/560070183650213889",
    ),
    d(
        "facebook",
        "Facebook",
        "social",
        &["facebook.com", "fb.watch"],
        "needs-login",
        &["需要登录状态的 Cookie，否则报 Cannot parse data"],
        "https://www.facebook.com/radiokicksfm/videos/3676516585958356/",
    ),
    d(
        "vimeo",
        "Vimeo",
        "video",
        &["vimeo.com"],
        "needs-login",
        &["web 客户端要求登录：选一个登录过 Vimeo 的浏览器 Cookie"],
        "https://vimeo.com/76979871",
    ),
    d(
        "dailymotion",
        "Dailymotion",
        "video",
        &["dailymotion.com", "dai.ly"],
        "ready",
        &["部分内容有地区限制"],
        "https://www.dailymotion.com/video/x5kesuj",
    ),
    d(
        "twitch",
        "Twitch",
        "live",
        &["twitch.tv"],
        "ready",
        &["点播（/videos/）可直接下；直播回放要等生成，直播中需要 Cookie"],
        "https://www.twitch.tv/videos/6528877",
    ),
    d(
        "nicovideo",
        "niconico",
        "video",
        &["nicovideo.jp", "nico.ms"],
        "ready",
        &["部分视频需要登录"],
        "https://www.nicovideo.jp/watch/sm9",
    ),
    d(
        "acfun",
        "AcFun",
        "anime",
        &["acfun.cn"],
        "ready",
        &["番剧区部分内容需要登录"],
        "https://www.acfun.cn/v/ac35457073",
    ),
    d(
        "huya",
        "虎牙",
        "live",
        &["huya.com"],
        "experimental",
        &["只支持已生成的录播（/video/play/），直播中拉流经常失败"],
        "https://www.huya.com/video/play/1002412640.html",
    ),
    d(
        "douyu",
        "斗鱼",
        "live",
        &["douyu.com"],
        "experimental",
        &["上游只支持录播页，直播源经常 404"],
        "https://www.douyu.com/572329",
    ),
    d(
        "ixigua",
        "西瓜视频",
        "video",
        &["ixigua.com"],
        "needs-cookies",
        &["必须带本机浏览器 Cookie"],
        "https://www.ixigua.com/6996881461559165471",
    ),
    d(
        "youku",
        "优酷",
        "video",
        &["youku.com"],
        "experimental",
        &["多数内容有账号风控（错误码 6004），需要真实的会员 Cookie"],
        "https://v.youku.com/v_show/id_XNjA1NzA2Njgw.html",
    ),
    d(
        "iqiyi",
        "爱奇艺",
        "video",
        &["iqiyi.com"],
        "experimental",
        &["上游支持很不稳定，经常 Can't find any video"],
        "https://www.iqiyi.com/v_19rrojlavg.html",
    ),
    d(
        "netease-music",
        "网易云音乐",
        "music",
        &["music.163.com"],
        "ready",
        &["部分歌曲有地区限制；VIP 曲目需要 Cookie"],
        "https://music.163.com/#/song?id=1901371647",
    ),
    d(
        "qqmusic",
        "QQ 音乐",
        "music",
        &["y.qq.com"],
        "ready",
        &["VIP / 独家曲目需要登录 Cookie"],
        "https://y.qq.com/n/ryqq/songDetail/004Ti8rT003TaZ",
    ),
    d(
        "kuwo",
        "酷我音乐",
        "music",
        &["kuwo.cn"],
        "experimental",
        &["上游解析经常失败（Unable to extract song name）"],
        "https://www.kuwo.cn/play_detail/635632",
    ),
    d(
        "ximalaya",
        "喜马拉雅",
        "music",
        &["ximalaya.com"],
        "ready",
        &["付费专辑需要 Cookie"],
        "https://www.ximalaya.com/sound/100000000",
    ),
    d(
        "soundcloud",
        "SoundCloud",
        "music",
        &["soundcloud.com"],
        "ready",
        &["Go+ 独家曲目需要 Cookie"],
        "https://soundcloud.com/forss/flickermood",
    ),
    d(
        "bandcamp",
        "Bandcamp",
        "music",
        &["bandcamp.com"],
        "ready",
        &["只给试听片段时无法拿到整曲"],
        "https://benprunty.bandcamp.com/track/lanius-battle",
    ),
    d(
        "archive",
        "Internet Archive",
        "other",
        &["archive.org"],
        "ready",
        &["完全公开，不需要 Cookie —— 可以用它验证「下载链路本身是否正常」"],
        "https://archive.org/details/BigBuckBunny_124",
    ),
    d(
        "peertube",
        "PeerTube",
        "other",
        &["framatube.org", "peertube2.cpy.re"],
        "ready",
        &["去中心化实例，不同实例能力不同"],
        "https://framatube.org/videos/watch/9c9de5e8-0a1e-484a-b099-e80766180a6d",
    ),
    d(
        "rumble",
        "Rumble",
        "video",
        &["rumble.com"],
        "experimental",
        &["页面有 Cloudflare 防护，先开指纹伪装或挂代理再试"],
        "https://rumble.com/v6d0xyz.html",
    ),
    d(
        "odysee",
        "Odysee",
        "video",
        &["odysee.com"],
        "ready",
        &["走 lbry 提取器，需用完整内容地址"],
        "https://odysee.com/@gardeningincanada:b/plants-i-will-never-grow-again.-the:e",
    ),
    d(
        "bitchute",
        "BitChute",
        "video",
        &["bitchute.com"],
        "ready",
        &["部分视频被下架后会解析失败"],
        "https://www.bitchute.com/video/UGlrF9o9b-Q/",
    ),
    d(
        "imgur",
        "Imgur",
        "other",
        &["imgur.com"],
        "experimental",
        &["相册/图集页通常没有视频流，返回 0 个格式"],
        "https://imgur.com/gallery/9v61V",
    ),
    d(
        "ted",
        "TED",
        "video",
        &["ted.com"],
        "ready",
        &["演讲页可直接下，含多语言字幕"],
        "https://www.ted.com/talks/bill_gates_the_next_outbreak_we_re_not_ready",
    ),
    d(
        "streamable",
        "Streamable",
        "other",
        &["streamable.com"],
        "ready",
        &["短链需要能访问原视频页"],
        "https://streamable.com/moo",
    ),
    d(
        "zhihu",
        "知乎",
        "social",
        &["zhihu.com"],
        "ready",
        &["只支持视频类回答（zvideo）"],
        "https://www.zhihu.com/zvideo/1342930761977176064",
    ),
    d(
        "vk",
        "VK",
        "social",
        &["vk.com", "vkvideo.ru"],
        "ready",
        &["私密视频需要 Cookie"],
        "https://vk.com/video-220754053_456239017",
    ),
];

fn to_info(d: &Def) -> SiteInfo {
    SiteInfo {
        key: d.key.to_string(),
        name: d.name.to_string(),
        category: d.category.to_string(),
        domains: d.domains.iter().map(|s| s.to_string()).collect(),
        status: d.status.to_string(),
        tips: d.tips.iter().map(|s| s.to_string()).collect(),
        sample_url: d.sample.to_string(),
    }
}

pub fn curated() -> Vec<SiteInfo> {
    SITES.iter().map(to_info).collect()
}

#[allow(dead_code)] // 供后续「按 key 取站点详情」用，目前 UI 走 probe 返回的 site
pub fn find(key: &str) -> Option<SiteInfo> {
    SITES.iter().find(|d| d.key == key).map(to_info)
}

/// 从链接里取小写主机名（不引入 url 库，够用就行）
fn host_of(url: &str) -> Option<String> {
    let s = url.trim();
    let rest = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?.to_lowercase();
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    Some(host)
}

/// 按域名匹配站点（同名域下再按 path_hint 细分）
pub fn match_url(url: &str) -> Option<SiteInfo> {
    let host = host_of(url)?;
    for d in SITES {
        if let Some(hint) = d.path_hint {
            if !url.contains(hint) {
                continue;
            }
        }
        for dom in d.domains {
            let dom = dom.to_lowercase();
            if host == dom || host.ends_with(&format!(".{dom}")) {
                return Some(to_info(d));
            }
        }
    }
    None
}

// ── 策略记忆 ───────────────────────────────────────────────────

fn store_path() -> std::path::PathBuf {
    crate::paths::config_dir().join("strategies.json")
}

pub fn strategies() -> BTreeMap<String, String> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(&t).ok())
        .unwrap_or_default()
}

pub fn strategy_for(key: &str) -> Option<String> {
    strategies().get(key).cloned()
}

/// 记住某站生效的非默认策略（默认策略不用记，那本来就是首选）
pub fn remember(key: &str, strategy: &str) {
    if strategy == "default" || key.is_empty() {
        return;
    }
    let mut m = strategies();
    if m.get(key).map(|s| s.as_str()) == Some(strategy) {
        return;
    }
    m.insert(key.to_string(), strategy.to_string());
    let p = store_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_vec_pretty(&m) {
        let _ = std::fs::write(&p, body);
    }
}
