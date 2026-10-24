use crate::client::{api_error, ApiClient};
use crate::video;
use crate::wbi;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const COMMENT_URL: &str = "https://api.bilibili.com/x/v2/reply/wbi/main";
const CANCELLED: &str = "采集已取消";
const PAGE_SLEEP_MS: u64 = 1000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentCrawlJob {
    pub id: String,
    pub bvid: String,
    pub aid: u64,
    pub title: String,
    pub mode: String,
    pub status: String,
    pub total: u64,
    pub roots: u64,
    pub subs: u64,
    pub pages: u32,
    pub current: String,
    pub note: Option<String>,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentVideo {
    pub bvid: String,
    pub title: String,
    pub total: u64,
    pub updated_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentStats {
    pub roots: u64,
    pub subs: u64,
    pub total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentRow {
    pub rpid: u64,
    pub root: u64,
    pub parent: u64,
    pub mid: u64,
    pub uname: String,
    pub avatar_url: String,
    pub message: String,
    pub like: u64,
    pub ctime: u64,
    pub is_sub: bool,
    pub rcount: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub count: u64,
}

struct JobEntry {
    job: CommentCrawlJob,
    cancel: CancellationToken,
}

pub struct CommentCrawlManager {
    jobs: Mutex<HashMap<String, JobEntry>>,
    db_path: PathBuf,
}

impl CommentCrawlManager {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            db_path,
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn list(&self) -> Result<Vec<CommentCrawlJob>, String> {
        let jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        let mut list: Vec<CommentCrawlJob> = jobs.values().map(|entry| entry.job.clone()).collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(list)
    }

    pub fn delete_video(&self, bvid: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        if jobs.values().any(|entry| entry.job.bvid == bvid && matches!(entry.job.status.as_str(), "queued" | "running")) {
            return Err("请先停止采集，等待任务结束后再删除记录".into());
        }
        CommentStore::open(self.db_path())?.delete_video(bvid)?;
        jobs.retain(|_, entry| entry.job.bvid != bvid);
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        let entry = jobs.get(id).ok_or("评论采集任务不存在")?;
        entry.cancel.cancel();
        Ok(())
    }

    fn update(&self, app: &AppHandle, id: &str, change: impl FnOnce(&mut CommentCrawlJob)) {
        let updated = {
            let mut jobs = match self.jobs.lock() {
                Ok(jobs) => jobs,
                Err(_) => return,
            };
            let Some(entry) = jobs.get_mut(id) else {
                return;
            };
            change(&mut entry.job);
            entry.job.clone()
        };
        let _ = app.emit("comment-progress", &updated);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    app: AppHandle,
    manager: Arc<CommentCrawlManager>,
    client: ApiClient,
    bvid: String,
    mode: String,
    max_pages: Option<u32>,
    logged_in: bool,
) -> Result<CommentCrawlJob, String> {
    let bvid = video::extract_bvid(&bvid).ok_or("无法识别 BV 号")?;
    let mode_code = if mode == "new" { 2 } else { 3 };
    let mode = if mode_code == 2 { "new" } else { "hot" };
    let max_pages = max_pages.filter(|pages| *pages > 0);

    {
        let jobs = manager.jobs.lock().map_err(|e| e.to_string())?;
        if jobs.values().any(|entry| {
            entry.job.bvid == bvid
                && matches!(entry.job.status.as_str(), "queued" | "running")
        }) {
            return Err("该视频已有评论采集任务在运行".into());
        }
    }

    let id = Uuid::new_v4().to_string();
    let job = CommentCrawlJob {
        id: id.clone(),
        bvid: bvid.clone(),
        aid: 0,
        title: String::new(),
        mode: mode.to_string(),
        status: "queued".into(),
        total: 0,
        roots: 0,
        subs: 0,
        pages: 0,
        current: "准备中".into(),
        note: None,
        error: None,
        started_at: now_ms(),
        finished_at: None,
    };
    let cancel = CancellationToken::new();
    {
        let mut jobs = manager.jobs.lock().map_err(|e| e.to_string())?;
        jobs.insert(
            id.clone(),
            JobEntry {
                job: job.clone(),
                cancel: cancel.clone(),
            },
        );
    }
    let _ = app.emit("comment-progress", &job);

    let manager_for_task = manager.clone();
    let app_for_task = app.clone();
    let id_for_task = id.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_crawl(
            &app_for_task,
            &manager_for_task,
            &client,
            &id_for_task,
            &bvid,
            mode_code,
            max_pages,
            logged_in,
            &cancel,
        )
        .await;
        match result {
            Ok(()) => {}
            Err(message) if message == CANCELLED => {
                manager_for_task.update(&app_for_task, &id_for_task, |job| {
                    job.status = "cancelled".into();
                    job.current = "已取消".into();
                    job.mark_finished();
                });
            }
            Err(message) => {
                manager_for_task.update(&app_for_task, &id_for_task, |job| {
                    job.status = "failed".into();
                    job.current = "采集失败".into();
                    job.error = Some(message);
                    job.finished_at = Some(now_ms());
                });
            }
        }
    });
    Ok(job)
}

impl CommentCrawlJob {
    fn mark_finished(&mut self) {
        self.finished_at = Some(now_ms());
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_crawl(
    app: &AppHandle,
    manager: &Arc<CommentCrawlManager>,
    client: &ApiClient,
    id: &str,
    bvid: &str,
    mode_code: u32,
    max_pages: Option<u32>,
    logged_in: bool,
    cancel: &CancellationToken,
) -> Result<(), String> {
    manager.update(app, id, |job| {
        job.status = "running".into();
        job.current = "获取视频信息".into();
    });

    let view = video::fetch_view(client, bvid).await?;
    let aid = view
        .get("aid")
        .and_then(Value::as_u64)
        .ok_or("视频信息缺少 aid")?;
    let title = view
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(bvid)
        .to_string();
    let store = CommentStore::open(manager.db_path())?;
    store.touch_video(bvid, aid, &title)?;
    manager.update(app, id, |job| {
        job.aid = aid;
        job.title = title.clone();
        job.current = "获取签名密钥".into();
    });

    let mut mixin_key = wbi::fetch_mixin_key(client).await?;
    let mut offset = String::new();
    let mut pages = 0u32;
    let mut roots = 0u64;
    let mut root_ids: HashSet<u64> = HashSet::new();
    let mut total = 0u64;
    let mut hit_limit = false;

    loop {
        if cancel.is_cancelled() {
            return Err(CANCELLED.into());
        }
        if let Some(limit) = max_pages {
            if pages >= limit {
                hit_limit = true;
                break;
            }
        }
        pages += 1;
        manager.update(app, id, |job| {
            job.current = format!("正在读取第 {pages} 页主评论");
        });

        let request = reply_params(aid, &offset, mode_code);
        let value = signed_get(client, COMMENT_URL, &request, &mut mixin_key).await?;
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            let message = value.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(api_error(code, message));
        }
        let data = value.get("data").ok_or("评论响应缺少 data")?;
        let cursor = data.get("cursor").cloned().unwrap_or(Value::Null);
        total = cursor
            .get("all_count")
            .and_then(Value::as_u64)
            .unwrap_or(total);

        let mut batch: Vec<Value> = Vec::new();
        if let Some(items) = data.get("top_replies").and_then(Value::as_array) {
            batch.extend(items.iter().cloned());
        }
        if let Some(top) = data.get("top") {
            for key in ["admin", "upper"] {
                if let Some(reply) = top.get(key) {
                    if reply.get("rpid").is_some() {
                        batch.push(reply.clone());
                    }
                }
            }
        }
        if let Some(items) = data.get("replies").and_then(Value::as_array) {
            batch.extend(items.iter().cloned());
        }

        for (index, reply) in batch.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(CANCELLED.into());
            }
            // Count unique comments processed in this run, not only new DB
            // inserts: re-crawling an existing video must still report N/N.
            let _ = store.save_reply(bvid, aid, reply, false)?;
            if let Some(rpid) = reply.get("rpid").and_then(Value::as_u64) {
                if rpid > 0 && root_ids.insert(rpid) {
                    roots += 1;
                }
            }
            manager.update(app, id, |job| {
                job.total = total;
                job.roots = roots;
                job.subs = 0;
                job.current = format!("第 {pages} 页 · 主评论 {}/{}", index + 1, batch.len());
            });
        }

        manager.update(app, id, |job| {
            job.total = total;
            job.roots = roots;
            job.subs = 0;
            job.pages = pages;
            job.current = format!("第 {pages} 页采集完成");
        });

        let is_end = cursor
            .get("is_end")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let next = cursor
            .pointer("/pagination_reply/next_offset")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if is_end || next.is_empty() {
            break;
        }
        offset = next;
        tokio::time::sleep(Duration::from_millis(PAGE_SLEEP_MS)).await;
    }

    let mut note = None;
    if !logged_in {
        note = Some("未登录：评论列表可能有可见上限".into());
    } else if hit_limit && (total == 0 || roots < total) {
        note = Some(format!(
            "已达到本次页数上限（{pages} 页），可扩大范围补采；补采从第 1 页开始，已有评论去重保留"
        ));
    } else if total > 0 && roots < total * 95 / 100 {
        note = Some(format!(
            "另有 {} 条未返回，可能是已删除/审核中/不可见评论",
            total.saturating_sub(roots)
        ));
    } else if total == 0 && roots == 0 {
        note = Some("评论区为空或当前不可见".into());
    }

    manager.update(app, id, |job| {
        job.status = "completed".into();
        job.total = total;
        job.roots = roots;
        job.subs = 0;
        job.pages = pages;
        job.current = "采集完成".into();
        job.note = note;
        job.finished_at = Some(now_ms());
    });
    Ok(())
}

async fn signed_get(
    client: &ApiClient,
    url: &str,
    params: &[(&str, String)],
    mixin_key: &mut String,
) -> Result<Value, String> {
    let mut refreshed = false;
    loop {
        let query = wbi::sign_query(params, mixin_key, wbi::now_ts());
        let value = client.get_raw_query(url, &query).await?;
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code == -352 && !refreshed {
            refreshed = true;
            *mixin_key = wbi::fetch_mixin_key(client).await?;
            continue;
        }
        return Ok(value);
    }
}

fn reply_params(aid: u64, offset: &str, mode: u32) -> Vec<(&'static str, String)> {
    vec![
        ("oid", aid.to_string()),
        ("type", "1".to_string()),
        ("mode", mode.to_string()),
        (
            "pagination_str",
            serde_json::json!({ "offset": offset }).to_string(),
        ),
        ("plat", "1".to_string()),
        ("seek_rpid", String::new()),
        ("web_location", "1315875".to_string()),
        (
            "x-bili-locale-json",
            r#"{"c_locale":{"language":"zh","script":"Hans"},"always_translate":false}"#.to_string(),
        ),
    ]
}

/// SQLite-backed comment storage. One connection guarded by a mutex; comment
/// batches are small and writes are synchronous, so this stays off the async
/// executor only for short periods.
pub struct CommentStore {
    conn: Mutex<Connection>,
}

impl CommentStore {
    pub fn videos(&self) -> Result<Vec<CommentVideo>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT v.bvid, v.title, COUNT(r.rpid),
                    MAX(v.last_seen_at, COALESCE(MAX(r.fetched_at), 0))
             FROM videos v LEFT JOIN replies r ON r.bvid = v.bvid AND r.is_sub = 0
             GROUP BY v.bvid ORDER BY 4 DESC, v.bvid ASC"
        ).map_err(|e| format!("读取评论记录失败：{e}"))?;
        let rows = stmt.query_map([], |row| Ok(CommentVideo {
            bvid: row.get(0)?,
            title: row.get(1)?,
            total: row.get::<_, i64>(2)? as u64,
            updated_at: row.get::<_, i64>(3)? as u64,
        })).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建评论数据目录：{e}"))?;
        }
        let mut conn = Connection::open(path).map_err(|e| format!("无法打开评论数据库：{e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS videos(
               bvid TEXT PRIMARY KEY,
               aid INTEGER NOT NULL,
               title TEXT NOT NULL,
               first_seen_at INTEGER NOT NULL,
               last_seen_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS replies(
               rpid INTEGER PRIMARY KEY,
               bvid TEXT NOT NULL,
               aid INTEGER NOT NULL,
               root INTEGER NOT NULL DEFAULT 0,
               parent INTEGER NOT NULL DEFAULT 0,
               mid INTEGER NOT NULL DEFAULT 0,
               uname TEXT NOT NULL DEFAULT '',
               message TEXT NOT NULL DEFAULT '',
               like_count INTEGER NOT NULL DEFAULT 0,
               ctime INTEGER NOT NULL DEFAULT 0,
               is_sub INTEGER NOT NULL DEFAULT 0,
               rcount INTEGER NOT NULL DEFAULT 0,
               raw TEXT NOT NULL,
               fetched_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_replies_bvid ON replies(bvid);
             CREATE INDEX IF NOT EXISTS idx_replies_root ON replies(root);",
        )
        .map_err(|e| format!("初始化评论数据库失败：{e}"))?;
        // Serialize migration across simultaneous readers opening the store.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let has_order: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('replies') WHERE name = 'collection_order')",
            [], |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        if !has_order {
            tx.execute_batch("ALTER TABLE replies ADD COLUMN collection_order INTEGER NOT NULL DEFAULT 0;
                WITH ordered AS (SELECT rpid, ROW_NUMBER() OVER (PARTITION BY bvid ORDER BY fetched_at, rpid) AS seq FROM replies)
                UPDATE replies SET collection_order = (SELECT seq FROM ordered WHERE ordered.rpid = replies.rpid);").map_err(|e| e.to_string())?;
        }
        tx.execute_batch("CREATE INDEX IF NOT EXISTS idx_replies_collection_order ON replies(bvid, collection_order);").map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn touch_video(&self, bvid: &str, aid: u64, title: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_ms().min(i64::MAX as u64) as i64;
        conn.execute(
            "INSERT INTO videos(bvid, aid, title, first_seen_at, last_seen_at)
             VALUES(?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(bvid) DO UPDATE SET
               aid=excluded.aid, title=excluded.title, last_seen_at=excluded.last_seen_at",
            params![bvid, aid as i64, title, now],
        )
        .map_err(|e| format!("写入视频信息失败：{e}"))?;
        Ok(())
    }

    pub fn save_reply(
        &self,
        bvid: &str,
        aid: u64,
        reply: &Value,
        is_sub: bool,
    ) -> Result<bool, String> {
        let rpid = reply.get("rpid").and_then(Value::as_u64).unwrap_or(0);
        if rpid == 0 {
            return Ok(false);
        }
        let root = reply.get("root").and_then(Value::as_u64).unwrap_or(0);
        let parent = reply.get("parent").and_then(Value::as_u64).unwrap_or(0);
        let member = reply.get("member");
        let content = reply.get("content");
        let mid = member
            .and_then(|m| m.get("mid"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let uname = member
            .and_then(|m| m.get("uname"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let message = content
            .and_then(|c| c.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let like = reply.get("like").and_then(Value::as_u64).unwrap_or(0);
        let ctime = reply.get("ctime").and_then(Value::as_u64).unwrap_or(0);
        let rcount = reply.get("rcount").and_then(Value::as_u64).unwrap_or(0);
        let raw = serde_json::to_string(&without_child_content(reply.clone())).unwrap_or_default();
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO replies(
                   rpid, bvid, aid, root, parent, mid, uname, message,
                   like_count, ctime, is_sub, rcount, raw, fetched_at, collection_order
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, (SELECT COALESCE(MAX(collection_order), 0) + 1 FROM replies WHERE bvid = ?2))",
                params![
                    rpid as i64,
                    bvid,
                    aid as i64,
                    root as i64,
                    parent as i64,
                    mid as i64,
                    uname,
                    message,
                    like as i64,
                    ctime as i64,
                    if is_sub { 1 } else { 0 },
                    rcount as i64,
                    raw,
                    now_ms().min(i64::MAX as u64) as i64,
                ],
            )
            .map_err(|e| format!("写入评论失败：{e}"))?;
        // Preserve the existing comment while refreshing its avatar on re-collection.
        if changed == 0 {
            let face = avatar_from_reply(reply);
            if !face.is_empty() {
                conn.execute(
                    "UPDATE replies SET raw = json_set(raw, '$.member.face', ?1, '$.member.avatar', ?1)
                     WHERE rpid = ?2 AND bvid = ?3 AND json_valid(raw)",
                    params![face, rpid as i64, bvid],
                ).map_err(|e| format!("更新头像链接失败：{e}"))?;
            }
        }
        Ok(changed > 0)
    }

    pub fn delete_video(&self, bvid: &str) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let transaction = conn.transaction().map_err(|e| e.to_string())?;
        transaction.execute("DELETE FROM replies WHERE bvid = ?1", [bvid]).map_err(|e| e.to_string())?;
        transaction.execute("DELETE FROM videos WHERE bvid = ?1", [bvid]).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| format!("删除评论记录失败：{e}"))
    }

    pub fn counts(&self, bvid: &str) -> Result<CommentStats, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let (total, roots, subs): (i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN is_sub = 0 THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN is_sub = 1 THEN 1 ELSE 0 END), 0)
                 FROM replies WHERE bvid = ?1",
                [bvid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| format!("读取评论统计失败：{e}"))?;
        Ok(CommentStats {
            roots: roots as u64,
            subs: subs as u64,
            total: total as u64,
        })
    }

    pub fn list(
        &self,
        bvid: &str,
        offset: u32,
        limit: u32,
        roots_only: bool,
    ) -> Result<Vec<CommentRow>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT rpid, root, parent, mid, uname, message, like_count, ctime, is_sub, rcount,
                        raw
                 FROM replies
                 WHERE bvid = ?1 AND (?2 = 0 OR is_sub = 0)
                 ORDER BY collection_order ASC, rpid ASC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![
                    bvid,
                    if roots_only { 1 } else { 0 },
                    limit as i64,
                    offset as i64
                ],
                |row| {
                    Ok(CommentRow {
                        rpid: row.get::<_, i64>(0)? as u64,
                        root: row.get::<_, i64>(1)? as u64,
                        parent: row.get::<_, i64>(2)? as u64,
                        mid: row.get::<_, i64>(3)? as u64,
                        uname: row.get(4)?,
                        avatar_url: avatar_from_raw(&row.get::<_, String>(10)?),
                        message: row.get(5)?,
                        like: row.get::<_, i64>(6)? as u64,
                        ctime: row.get::<_, i64>(7)? as u64,
                        is_sub: row.get::<_, i64>(8)? != 0,
                        rcount: row.get::<_, i64>(9)? as u64,
                    })
                },
            )
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取评论失败：{e}"))
    }

    pub fn export_csv(&self, bvid: &str, path: &Path) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT rpid, root, parent, mid, uname, message, like_count, ctime, is_sub, rcount,
                        raw
                 FROM replies WHERE bvid = ?1 AND is_sub = 0
                 ORDER BY collection_order ASC, rpid ASC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([bvid]).map_err(|e| e.to_string())?;
        let file = std::fs::File::create(path).map_err(|e| format!("无法创建导出文件：{e}"))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(b"\xEF\xBB\xBF")
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
        writeln!(writer, "rpid,root,parent,mid,uname,message,like,ctime,is_sub,rcount,avatar_url")
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
        let mut count = 0u64;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let rpid: i64 = row.get(0).map_err(|e| e.to_string())?;
            let root: i64 = row.get(1).map_err(|e| e.to_string())?;
            let parent: i64 = row.get(2).map_err(|e| e.to_string())?;
            let mid: i64 = row.get(3).map_err(|e| e.to_string())?;
            let uname: String = row.get(4).map_err(|e| e.to_string())?;
            let message: String = row.get(5).map_err(|e| e.to_string())?;
            let like: i64 = row.get(6).map_err(|e| e.to_string())?;
            let ctime: i64 = row.get(7).map_err(|e| e.to_string())?;
            let is_sub: i64 = row.get(8).map_err(|e| e.to_string())?;
            let rcount: i64 = row.get(9).map_err(|e| e.to_string())?;
            let avatar_url = avatar_from_raw(&row.get::<_, String>(10).map_err(|e| e.to_string())?);
            writeln!(
                writer,
                "{rpid},{root},{parent},{mid},{},{},{like},{ctime},{is_sub},{rcount},{}",
                csv_field(&uname),
                csv_field(&message),
                csv_field(&avatar_url)
            )
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
            count += 1;
        }
        writer
            .flush()
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
        Ok(count)
    }

    pub fn export_json(&self, bvid: &str, path: &Path) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT raw FROM replies WHERE bvid = ?1 AND is_sub = 0
                 ORDER BY collection_order ASC, rpid ASC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([bvid]).map_err(|e| e.to_string())?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let raw: String = row.get(0).map_err(|e| e.to_string())?;
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                values.push(without_child_content(value));
            }
        }
        let file = std::fs::File::create(path).map_err(|e| format!("无法创建导出文件：{e}"))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &values)
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
        writer
            .flush()
            .map_err(|e| format!("写入导出文件失败：{e}"))?;
        Ok(values.len() as u64)
    }
}

fn avatar_from_reply(value: &Value) -> String {
    ["/member/avatar", "/member/face"].iter()
        .filter_map(|path| value.pointer(path).and_then(Value::as_str))
        .find(|url| !url.trim().is_empty())
        .unwrap_or("").trim().to_string()
}

fn avatar_from_raw(raw: &str) -> String {
    serde_json::from_str::<Value>(raw).map(|value| avatar_from_reply(&value)).unwrap_or_default()
}

// The list endpoint may embed child previews; retain only their count.
fn without_child_content(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("replies");
    }
    value
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_current_and_legacy_avatar_fields() {
        assert_eq!(avatar_from_raw(r#"{"member":{"avatar":"https://example.com/current.jpg"}}"#), "https://example.com/current.jpg");
        assert_eq!(avatar_from_raw(r#"{"member":{"face":"https://example.com/legacy.jpg"}}"#), "https://example.com/legacy.jpg");
        assert_eq!(avatar_from_raw(r#"{"member":{"avatar":"","face":"https://example.com/legacy.jpg"}}"#), "https://example.com/legacy.jpg");
        assert_eq!(avatar_from_raw("invalid"), "");
    }

    #[test]
    fn collection_order_survives_recollection_and_pagination() {
        let dir = std::env::temp_dir().join(format!("biliscope-sort-{}", Uuid::new_v4()));
        let store = CommentStore::open(&dir.join("comments.db")).unwrap();
        for (id, time, likes) in [(1, 100, 50), (2, 200, 1), (3, 200, 1)] {
            store.save_reply("BVTEST", 1, &serde_json::json!({"rpid":id,"ctime":time,"like":likes}), false).unwrap();
        }
        assert_eq!(store.list("BVTEST", 0, 1, true).unwrap()[0].rpid, 1);
        assert_eq!(store.list("BVTEST", 1, 1, true).unwrap()[0].rpid, 2);
        store.save_reply("BVTEST", 1, &serde_json::json!({"rpid":1,"like":999}), false).unwrap();
        assert_eq!(store.list("BVTEST", 0, 1, true).unwrap()[0].rpid, 1);
        assert_eq!(store.list("BVTEST", 2, 1, true).unwrap()[0].rpid, 3);
    }

    #[test]
    fn csv_quotes_special_fields() {
        assert_eq!(csv_field("hello"), "hello");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
    }

    #[test]
    fn writes_and_counts_comments_in_a_temp_database() {
        let dir = std::env::temp_dir().join(format!("biliscope-comments-test-{}", Uuid::new_v4()));
        let store = CommentStore::open(&dir.join("comments.db")).unwrap();
        store.touch_video("BV1TEST", 42, "测试视频").unwrap();
        let reply = serde_json::json!({
            "rpid": 1001,
            "root": 0,
            "parent": 0,
            "member": { "mid": 7, "uname": "测试用户", "avatar": "https://example.com/avatar.jpg" },
            "content": { "message": "第一条评论" },
            "like": 9,
            "ctime": 1700000000,
            "rcount": 2,
            "replies": [{"rpid": 1002, "content": {"message": "楼中楼"}}]
        });
        assert!(store.save_reply("BV1TEST", 42, &reply, false).unwrap());
        assert!(!store.save_reply("BV1TEST", 42, &reply, false).unwrap());
        let sub = serde_json::json!({
            "rpid": 1002,
            "root": 1001,
            "parent": 1001,
            "member": { "mid": 8, "uname": "回复者" },
            "content": { "message": "楼中楼" },
            "like": 1,
            "ctime": 1700000001,
            "rcount": 0
        });
        assert!(store.save_reply("BV1TEST", 42, &sub, true).unwrap());
        let stats = store.counts("BV1TEST").unwrap();
        assert_eq!((stats.roots, stats.subs, stats.total), (1, 1, 2));
        let rows = store.list("BV1TEST", 0, 10, true).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uname, "测试用户");
        assert_eq!(rows[0].avatar_url, "https://example.com/avatar.jpg");
        let all = store.list("BV1TEST", 0, 10, false).unwrap();
        assert!(all.iter().find(|row| row.is_sub).unwrap().avatar_url.is_empty());

        let csv_path = dir.join("comments.csv");
        assert_eq!(store.export_csv("BV1TEST", &csv_path).unwrap(), 1);
        let csv = std::fs::read_to_string(&csv_path).unwrap();
        assert!(csv.contains("rpid,root,parent"));
        assert!(csv.contains("第一条评论"));
        assert!(csv.contains("avatar_url"));
        assert!(csv.contains("https://example.com/avatar.jpg"));
        assert!(!csv.contains("楼中楼"));

        let json_path = dir.join("comments.json");
        assert_eq!(store.export_json("BV1TEST", &json_path).unwrap(), 1);
        let json: Value =
            serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["rcount"], 2);
        assert_eq!(json[0]["member"]["avatar"], "https://example.com/avatar.jpg");
        let mut updated = reply.clone();
        updated["member"]["avatar"] = Value::String("https://example.com/new-avatar.jpg".into());
        assert!(!store.save_reply("BV1TEST", 42, &updated, false).unwrap());
        assert_eq!(store.list("BV1TEST", 0, 10, true).unwrap()[0].avatar_url, "https://example.com/new-avatar.jpg");
        assert!(json[0].get("replies").is_none());
        assert_eq!(rows[0].rcount, 2);

        // Library entries survive process restarts, include empty videos, and
        // count only root replies without duplicating re-crawls.
        store.touch_video("BVEMPTY", 43, "尚无评论的视频").unwrap();
        drop(store);
        let reopened = CommentStore::open(&dir.join("comments.db")).unwrap();
        let videos = reopened.videos().unwrap();
        assert_eq!(videos.len(), 2);
        let saved = videos.iter().find(|video| video.bvid == "BV1TEST").unwrap();
        assert_eq!(saved.title, "测试视频");
        assert_eq!(saved.total, 1);
        assert!(saved.updated_at > 0);
        assert_eq!(videos.iter().find(|video| video.bvid == "BVEMPTY").unwrap().total, 0);
        reopened.delete_video("BV1TEST").unwrap();
        assert_eq!(reopened.counts("BV1TEST").unwrap().total, 0);
        assert!(reopened.list("BV1TEST", 0, 20, false).unwrap().is_empty());
        assert_eq!(reopened.videos().unwrap().len(), 1);
        assert_eq!(reopened.videos().unwrap()[0].bvid, "BVEMPTY");
        reopened.delete_video("BV1TEST").unwrap();
        drop(reopened);

        let _ = std::fs::remove_dir_all(dir);
    }
}
