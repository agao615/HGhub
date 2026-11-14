//! 2048 AI 短剧（第二数据源）。
//!
//! 与黄果不同，2048 的接口是普通的 REST JSON（`/api/v1/**`），没有信封与加密：
//!
//! * 列表 `GET /api/v1/short-dramas?productId=1&sortBy=latest&page=&size=` → `items`
//! * 详情 `GET /api/v1/short-dramas/{id}?productId=1` → `episodes[{episodeNo,videoUrl,durationSec}]`
//! * 播放 `GET /api/v1/m3u8/proxy?path=<相对 m3u8>`（这是站点自己的代理，返回绝对分片地址）
//!
//! 这里把结果映射成黄果的 `VideoCard` / `Detail` / `PlayInfo` 形状，
//! 前端复用同一套 Feed、封面与 HLS 代理，不必再写一套播放器。
//! 封面与分片都不校验 Referer，沿用 media.rs 的请求头即可。

use crate::net::{client, err_text};
use serde_json::{json, Value};
use std::time::Duration;

pub const BASE: &str = "https://2048ai.vip";
/// 站点默认产品线（网页端固定 productId=1）
const PRODUCT: u32 = 1;
/// 列表排序：最近更新
const SORT: &str = "latest";

fn abs_url(u: &str) -> String {
    if u.is_empty() {
        return String::new();
    }
    if u.starts_with("http://") || u.starts_with("https://") {
        return u.to_string();
    }
    if u.starts_with('/') {
        format!("{BASE}{u}")
    } else {
        format!("{BASE}/{u}")
    }
}

/// 取 `{ code, message, data }` 里的 data，code != 200 视为失败。
async fn get(path: &str) -> Result<Value, String> {
    let url = format!("{BASE}{path}");
    let resp = client()
        .get(&url)
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/json")
        .header("Referer", format!("{BASE}/media/"))
        .send()
        .await
        .map_err(|e| err_text(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("2048 接口 HTTP {status}"));
    }
    let text = resp.text().await.map_err(|e| err_text(&e))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(1);
    if code != 200 {
        let msg = v
            .get("message")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .unwrap_or("2048 接口返回失败");
        return Err(msg.to_string());
    }
    Ok(v.get("data").cloned().unwrap_or(Value::Null))
}

/// 卡片：映射成黄果的 VideoCard
fn card(v: &Value) -> Value {
    let count = v.get("episodeCount").cloned().unwrap_or(json!(0));
    json!({
        "id": v.get("id").cloned().unwrap_or(json!(0)),
        "title": v.get("title").cloned().unwrap_or(json!("")),
        "cover": abs_url(v.get("coverUrl").and_then(|x| x.as_str()).unwrap_or("")),
        "description": v.get("description").cloned().unwrap_or(json!("")),
        "tags": [],
        "score": v.get("rating").cloned().unwrap_or(json!(0)),
        "episode_count": count,
        "total_episodes": count,
        "duration": "",
        "hot": v.get("heatCount").cloned().unwrap_or(json!(0)),
        "like_count": 0,
        "collect_count": 0,
        "comment_count": 0,
        "is_finished": false,
        "is_original": 1,
        "created_at": v.get("publishedAt").cloned().unwrap_or(json!("")),
        "author": { "id": -1, "name": "2048 AI短剧" }
    })
}

fn episodes(d: &Value) -> Vec<Value> {
    d.get("episodes")
        .and_then(|e| e.as_array())
        .map(|list| {
            list.iter()
                .map(|e| {
                    json!({
                        "ep_num": e.get("episodeNo").cloned().unwrap_or(json!(0)),
                        "title": e
                            .get("titleOverride")
                            .and_then(|t| t.as_str())
                            .or_else(|| e.get("title").and_then(|t| t.as_str()))
                            .unwrap_or("")
                            .to_string(),
                        "duration": e.get("durationSec").cloned().unwrap_or(json!(0)),
                        "status": 1
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 列表（分页）
pub async fn feed(page: u32, size: u32) -> Result<Value, String> {
    let data = get(&format!(
        "/api/v1/short-dramas?productId={PRODUCT}&sortBy={SORT}&page={page}&size={size}"
    ))
    .await?;
    let items = data
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mapped: Vec<Value> = items.iter().map(card).collect();
    Ok(json!({
        "items": mapped,
        "pagination": {
            "page": data.get("page").cloned().unwrap_or(json!(page)),
            "pages": data.get("totalPages").cloned().unwrap_or(json!(1)),
            "size": data.get("size").cloned().unwrap_or(json!(size)),
            "total": data.get("total").cloned().unwrap_or(json!(mapped.len())),
        }
    }))
}

/// 详情：映射成黄果的 Detail
pub async fn detail(id: i64) -> Result<Value, String> {
    let d = get(&format!("/api/v1/short-dramas/{id}?productId={PRODUCT}")).await?;
    if d.is_null() {
        return Err("剧集不存在".to_string());
    }
    let eps = episodes(&d);
    Ok(json!({
        "id": id,
        "title": d.get("title").cloned().unwrap_or(json!("")),
        "description": d.get("description").cloned().unwrap_or(json!("")),
        "cover": abs_url(d.get("coverUrl").and_then(|x| x.as_str()).unwrap_or("")),
        "tags": [],
        "score": d.get("rating").cloned().unwrap_or(json!(0)),
        "episode_count": eps.len(),
        "total_episodes": d.get("episodeCount").cloned().unwrap_or(json!(eps.len())),
        "episodes": eps,
        "counts": {
            "collect": 0,
            "comment": 0,
            "hot": d.get("heatCount").cloned().unwrap_or(json!(0)),
            "like": 0
        },
        "author": { "id": -1, "name": "2048 AI短剧", "is_following": false },
        "interaction": { "collected": false, "liked": false },
        "is_finished": false,
        "is_original": 1
    }))
}

/// 播放地址：站点自己的 m3u8 代理，前端再套一层本地协议
pub async fn play(id: i64, ep: u32) -> Result<Value, String> {
    let d = get(&format!("/api/v1/short-dramas/{id}?productId={PRODUCT}")).await?;
    let list = d
        .get("episodes")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    let target = list
        .iter()
        .find(|e| e.get("episodeNo").and_then(|n| n.as_u64()) == Some(ep as u64));
    let rel = target
        .and_then(|e| e.get("videoUrl"))
        .and_then(|u| u.as_str())
        .unwrap_or("");
    if rel.is_empty() {
        return Err("该集暂无播放地址".to_string());
    }
    let path: String = url::form_urlencoded::byte_serialize(rel.as_bytes()).collect();
    Ok(json!({
        "id": id,
        "ep_num": ep,
        "title": target
            .and_then(|e| e.get("title"))
            .cloned()
            .unwrap_or(json!("")),
        "duration": target
            .and_then(|e| e.get("durationSec"))
            .cloned()
            .unwrap_or(json!(0)),
        "video_url": format!("{BASE}/api/v1/m3u8/proxy?path={path}"),
    }))
}

/* ───────────────────────── 命令 ───────────────────────── */

#[tauri::command]
pub async fn api_2048_feed(page: u32, size: u32) -> Result<Value, String> {
    feed(page, size).await
}

#[tauri::command]
pub async fn api_2048_video(id: i64) -> Result<Value, String> {
    detail(id).await
}

#[tauri::command]
pub async fn api_2048_play(id: i64, ep: u32) -> Result<Value, String> {
    play(id, ep).await
}
