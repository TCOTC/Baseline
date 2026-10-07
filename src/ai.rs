//! AI 判定：拿不准「这条记录算哪条规则」的时候，让模型在**你自己写的那几条规则**里挑一条。
//!
//! # 它做什么，不做什么
//!
//! 只做一件事：从给定的候选里挑一条，或者回答「都不像」。
//!
//! - **不能新增规则、不能改写规则。** 候选是调用方从库里取出来的，
//!   模型的回答里出现候选之外的 id 一律作废（见 [`parse_reply`]）。
//! - **不能盖掉人已经说出口的话。** 人挑过的目标，`assist` 直接跳过。
//! - **不自信就不答。** 置信度低于门槛时什么都不选，界面把气泡亮着让人自己点——
//!   猜错就是一条假曲线，而假曲线正是这个产品要防的东西。
//!
//! 这一段以前是明确的非目标（「不替你判断该做什么」）。现在它仍然不是：
//! 它判的不是「你该做什么」，而是「你刚才做的这件事，落在你写的那句话里吗」。
//! 规则是人写的，候选是人给的，它只负责把那句话和这件事对上。
//!
//! # 密钥怎么存，以及这到底防住了什么
//!
//! 密钥以 ChaCha20-Poly1305 加密后落库，口令编译在二进制里（[`APP_SECRET`]）。
//!
//! **这不是安全边界，是提高门槛**：能读你的库的人，通常也能读这个 exe，
//! 所以拿得到口令。它真正挡住的是**按模式扫文件的窃取**——到处翻 `sk-` 开头的字符串
//! 的那种恶意软件，在密文上什么也找不到。
//!
//! 想要真正的保护应该用 Windows DPAPI（只有当前用户能解开，口令不在二进制里）。
//! 那需要 `windows-sys`，而它已经在依赖树里——换成它是一次很小的改动。

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::db;

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
pub const DEFAULT_MODEL: &str = "deepseek-chat";
/// 默认置信度门槛（0–100）。低于它的答案一律不用。
///
/// 定得偏高是故意的：这个数每降一点，曲线里就多一分「AI 说的」，
/// 而它一旦落进快照就冻结了。
pub const DEFAULT_THRESHOLD: u8 = 80;

/// 编译在二进制里的加密口令。见模块顶部：这是提高门槛，不是安全边界。
const APP_SECRET: &[u8; 32] = b"baseline/local/ai-key/v1/0000000";

const K_ENABLED: &str = "ai.enabled";
const K_BASE_URL: &str = "ai.base_url";
const K_MODEL: &str = "ai.model";
const K_KEY: &str = "ai.key";
const K_THRESHOLD: &str = "ai.threshold";

/// 一个请求最多等多久。判定卡在网络上比判定失败更糟：失败还能让人自己选，
/// 卡住则连「记一条」这件事都做不成。
const TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------- 配置

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    /// 明文，只在内存里。落库的是密文。
    pub api_key: String,
    pub threshold: u8,
    /// 库里那把密钥解不开（换了机器、或者密文被改过）。界面要能说出来。
    pub key_broken: bool,
}

impl Config {
    /// 现在能不能真的发请求。
    pub fn ready(&self) -> bool {
        self.enabled
            && !self.api_key.trim().is_empty()
            && !self.base_url.trim().is_empty()
            && !self.model.trim().is_empty()
    }

    /// 给界面看的密钥掩码。**明文绝不出内核**——页面是 HTML，会被截图、会被复制。
    pub fn masked_key(&self) -> String {
        mask(&self.api_key)
    }
}

pub fn mask(key: &str) -> String {
    let k = key.trim();
    if k.is_empty() {
        return String::new();
    }
    let n = k.chars().count();
    if n <= 4 {
        return "•".repeat(n.max(4));
    }
    let tail: String = k.chars().skip(n - 4).collect();
    format!("••••••••{tail}")
}

pub fn config(conn: &Connection) -> Result<Config> {
    let get = |k: &str| -> Result<Option<String>> { db::setting_get(conn, k) };
    let text = |k: &str, d: &str| -> Result<String> {
        Ok(get(k)?.unwrap_or_else(|| d.to_string()).trim().to_string())
    };

    let (api_key, key_broken) = match get(K_KEY)? {
        Some(blob) if !blob.trim().is_empty() => match open(&blob) {
            Ok(k) => (k, false),
            // 解不开就当没配：**不能因此让「记一条」失败**。
            Err(_) => (String::new(), true),
        },
        _ => (String::new(), false),
    };

    Ok(Config {
        enabled: get(K_ENABLED)?.map(|v| v != "0").unwrap_or(true),
        base_url: text(K_BASE_URL, DEFAULT_BASE_URL)?,
        model: text(K_MODEL, DEFAULT_MODEL)?,
        api_key,
        threshold: get(K_THRESHOLD)?
            .and_then(|v| v.parse::<u8>().ok())
            .map(|v| v.min(100))
            .unwrap_or(DEFAULT_THRESHOLD),
        key_broken,
    })
}

/// 保存设置。`api_key` 为 `None` 表示不动原来那把；`Some("")` 表示清掉。
pub fn save(
    conn: &Connection,
    enabled: bool,
    base_url: &str,
    model: &str,
    threshold: u8,
    api_key: Option<&str>,
) -> Result<()> {
    db::setting_set(conn, K_ENABLED, if enabled { "1" } else { "0" })?;
    db::setting_set(conn, K_BASE_URL, base_url.trim())?;
    db::setting_set(conn, K_MODEL, model.trim())?;
    db::setting_set(conn, K_THRESHOLD, &threshold.min(100).to_string())?;
    if let Some(k) = api_key {
        let k = k.trim();
        if k.is_empty() {
            db::setting_del(conn, K_KEY)?;
        } else {
            db::setting_set(conn, K_KEY, &seal(k)?)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 密钥加解密

/// 密文格式：`hex(nonce ‖ ciphertext)`。nonce 每次重新随机——
/// 同一个口令下重用 nonce 会让两把密钥的异或泄露出去。
pub fn seal(plain: &str) -> Result<String> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(APP_SECRET));
    let mut n = [0u8; 12];
    // getrandom 的 Error 没有实现 std::error::Error（它要尽量不依赖 std），
    // 所以这里手动转一层，用不上 anyhow 的 context。
    getrandom::fill(&mut n).map_err(|e| anyhow!("取随机数失败：{e}"))?;
    let nonce = Nonce::from_slice(&n);
    let ct = cipher
        .encrypt(nonce, plain.as_bytes())
        .map_err(|_| anyhow!("加密密钥失败"))?;
    let mut blob = n.to_vec();
    blob.extend_from_slice(&ct);
    Ok(hex_encode(&blob))
}

pub fn open(blob: &str) -> Result<String> {
    let raw = hex_decode(blob.trim()).ok_or_else(|| anyhow!("密文不是合法的十六进制"))?;
    if raw.len() < 13 {
        bail!("密文太短");
    }
    let (n, ct) = raw.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(APP_SECRET));
    let pt = cipher
        .decrypt(Nonce::from_slice(n), ct)
        .map_err(|_| anyhow!("密钥解不开（换过机器，或者密文被改过）"))?;
    String::from_utf8(pt).context("解出来的不是文本")
}

fn hex_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

// ---------------------------------------------------------------- 判定

/// 一条候选规则。`what` 是人写的那句话本身，原样交给模型。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub what: String,
}

/// 模型的回答，**已经按候选核对过**。
#[derive(Debug, Clone, Default)]
pub struct Pick {
    /// 挑中的规则。`None` = 都不像，或者它给的 id 不在候选里。
    pub source_id: Option<i64>,
    /// 0–100。`source_id` 为 None 时恒为 0。
    pub confidence: u8,
}

/// 一次补判的结果。
#[derive(Debug, Default)]
pub struct Filled {
    /// 补上了几条归属。
    pub decided: usize,
    /// 出问题时的原话（发请求失败之类）。没配 AI 或者没什么可判时是 None。
    pub trouble: Option<String>,
}

/// 给**一条**记录补上归属。
///
/// 两种情况靠同一个入口：
///
/// - 挂到某个目标上了，但没归到规则（`source_id IS NULL`）——在那个目标的规则里挑一条；
/// - 压根没挂目标——**在所有目标的规则里挑一条，目标也一并定下来**。
///
/// 第二种以前没有：那时候「记一条」不选目标就真的只是记下来，谁也不会再动它。
/// 「归到哪个目标是第二步」这句话里的第二步，本来就不该永远是人工的。
///
/// 判得准才写；写的是 `source_id IS NULL` 的格子，或者一条还不存在的关联。
/// 人挑过的、唯一能确定的那条，它碰不到。
pub fn classify_one(conn: &Connection, checkin_id: i64, today: NaiveDate) -> Result<Filled> {
    let cfg = config(conn)?;
    if !cfg.ready() {
        // 没配 AI 不是错误：那些记录本来就等着人指认。
        return Ok(Filled::default());
    }

    let links = db::links_of(conn, checkin_id)?;
    let (cands, owner) = if links.is_empty() {
        // 没挂目标：候选是全库的手工规则，标签带上目标名——
        // 不然「读完一章」这种规则名在几个目标下都长得一样，没法区分。
        all_rules(conn)?
    } else {
        // 挂了目标：只在这个目标自己的规则里挑。一个目标一条规则的时候唯一确定，
        // 轮不到模型；一条都没有时它没有资格说话（那是「我本来想推进它」）。
        let mut cands = Vec::new();
        let mut owner = std::collections::HashMap::new();
        for l in &links {
            if l.source_id.is_some() {
                continue; // 这一格已经有人定了
            }
            let srcs = db::manual_sources_of(conn, l.goal_id)?;
            if srcs.len() < 2 {
                continue;
            }
            for s in srcs {
                cands.push(Candidate {
                    id: s.id,
                    what: s.summary(),
                });
                owner.insert(s.id, l.goal_id);
            }
        }
        (cands, owner)
    };
    if cands.is_empty() {
        return Ok(Filled::default());
    }

    let mut out = Filled::default();
    let note = db::checkin_note(conn, checkin_id)?;
    match classify(&cfg, &note, &cands) {
        Ok(p) if p.confidence >= cfg.threshold => {
            if let (Some(sid), Some(gid)) = (p.source_id, p.source_id.and_then(|s| owner.get(&s))) {
                db::attribute(conn, checkin_id, *gid, sid, true)?;
                out.decided += 1;
            }
        }
        Ok(_) => {}
        Err(e) => out.trouble = Some(format!("AI 没判成：{e}")),
    }

    // 补上归属之后必须重算快照：数字和曲线都读它。
    // 今天的那一格本来就每次重算，所以这条记录当场就会进曲线。
    if out.decided > 0 {
        crate::metrics::roll(conn, today)?;
    }
    Ok(out)
}

/// 全库的手工规则，标签是「目标名 / 规则名」。返回 (候选, 规则 id → 目标 id)。
fn all_rules(
    conn: &Connection,
) -> Result<(Vec<Candidate>, std::collections::HashMap<i64, i64>)> {
    let mut cands = Vec::new();
    let mut owner = std::collections::HashMap::new();
    for g in db::goal_list(conn, false)? {
        for s in db::manual_sources_of(conn, g.id)? {
            cands.push(Candidate {
                id: s.id,
                what: format!("{} / {}", g.title, s.summary()),
            });
            owner.insert(s.id, g.id);
        }
    }
    Ok((cands, owner))
}

/// 某个目标下挂空着的记录，挨条补判。详情页那个「让 AI 判这 N 条」走它。
pub fn classify_goal_backlog(
    conn: &Connection,
    goal_id: i64,
    today: NaiveDate,
) -> Result<Filled> {
    let mut out = Filled::default();
    for (checkin_id, _) in db::unattributed_of(conn, goal_id)? {
        merge(&mut out, classify_one(conn, checkin_id, today)?);
    }
    Ok(out)
}

/// 没挂到任何目标上的记录，挨条补判。主视图那条「有 N 条没关联目标」走它。
pub fn classify_unlinked_backlog(conn: &Connection, today: NaiveDate) -> Result<Filled> {
    let mut out = Filled::default();
    for (checkin_id, _) in db::unlinked_checkins(conn)? {
        merge(&mut out, classify_one(conn, checkin_id, today)?);
    }
    Ok(out)
}

fn merge(into: &mut Filled, one: Filled) {
    into.decided += one.decided;
    if into.trouble.is_none() {
        into.trouble = one.trouble;
    }
}

/// 把一件事和几条规则交给模型，问它落在哪一条里。
pub fn classify(cfg: &Config, note: &str, cands: &[Candidate]) -> Result<Pick> {
    if cands.is_empty() {
        return Ok(Pick::default());
    }
    let url = format!(
        "{}/chat/completions",
        cfg.base_url.trim().trim_end_matches('/')
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();

    let resp = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", cfg.api_key.trim()))
        .header("Content-Type", "application/json")
        .send_json(build_request(cfg, note, cands))
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => match code {
                401 | 403 => anyhow!("密钥被拒了（HTTP {code}），去设置里看看密钥对不对"),
                404 => anyhow!("接口不存在（HTTP 404），看看 base URL 是不是少了一段"),
                429 => anyhow!("被限流了（HTTP 429），等一会儿再试"),
                c => anyhow!("服务返回 HTTP {c}"),
            },
            ureq::Error::Timeout(_) => anyhow!("等太久了（超过 {} 秒）", TIMEOUT.as_secs()),
            other => anyhow!("请求发不出去：{other}"),
        })?;

    let body: Value = resp
        .into_body()
        .read_json()
        .context("服务返回的不是 JSON")?;
    parse_reply(&body, cands)
}

/// 请求体。**没有用 `response_format`**：它虽然能逼出 JSON，但不是所有
/// OpenAI 兼容服务都实现了，用了就会在那些服务上直接 400。
/// 与其挑服务，不如把提示词写清楚、把解析写宽容（见 [`parse_reply`]）。
pub fn build_request(cfg: &Config, note: &str, cands: &[Candidate]) -> Value {
    let list: Vec<String> = cands
        .iter()
        .map(|c| format!("- id = {} ： {}", c.id, c.what))
        .collect();
    json!({
        "model": cfg.model,
        // 这是归类，不是创作。温度调 0，让同一个输入尽量给出同一个答案。
        "temperature": 0,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": format!(
                "刚做完的事：\n{}\n\n候选规则（只能从这里挑）：\n{}",
                note.trim(),
                list.join("\n")
            ) }
        ]
    })
}

const SYSTEM_PROMPT: &str = "\
你在帮一个人把他刚做的事归到他自己的判定规则里。这些规则是他写的，用来决定哪条曲线会动。

规矩：
1. 只能从候选里挑一条，或者回答 null。不能新增规则，不能改写规则，不能挑候选外的 id。
2. 判断标准只有一条：这件事是否落在那条规则写明的范围里。不要联想、不要类推。
3. 只要没有任何一条明确匹配，就回答 null。**宁可说不知道，也不要挑一个最像的**——
   挑错会让他以为那条线动了，而他做这个工具就是为了不被骗。
4. 只输出 JSON，不要解释，不要代码块：
{\"id\": 候选里的 id 或 null, \"confidence\": 0 到 100 的整数}

confidence 是你对这次判断的把握。不确定就写低一点，写低了不会被采用。";

/// 解析模型的回答。
///
/// **核对是这里做的**：模型可能编一个没给它的 id、可能把 JSON 包在代码块里、
/// 可能干脆回一段话。这三种都不能变成一条归属。
pub fn parse_reply(body: &Value, cands: &[Candidate]) -> Result<Pick> {
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("回复里没有 choices[0].message.content"))?;

    let raw = extract_json(content)
        .ok_or_else(|| anyhow!("回复里找不到 JSON：{}", brief(content)))?;
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow!("回复不是合法 JSON（{e}）：{}", brief(content)))?;

    let conf = v
        .get("confidence")
        .and_then(|c| c.as_i64())
        .unwrap_or(0)
        .clamp(0, 100) as u8;

    // 候选之外的 id 一律作废。这一条比什么都重要：归属是写进库、并且会冻住的东西。
    let source_id = v
        .get("id")
        .and_then(|i| i.as_i64())
        .filter(|i| cands.iter().any(|c| c.id == *i));

    Ok(Pick {
        source_id,
        confidence: if source_id.is_some() { conf } else { 0 },
    })
}

/// 从一段可能带代码块、带前言后语的文本里抠出那个 JSON 对象。
fn extract_json(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(s[start..=end].to_string())
}

fn brief(s: &str) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() > 120 {
        format!("{}…", t.chars().take(120).collect::<String>())
    } else {
        t
    }
}

/// 设置页上那个「测一下」。它要回答的是「配好没有」，所以真发一个最小的请求，
/// 并且把**用的是哪个模型**报回来——配错了模型是最常见的一种「看起来配好了」。
pub fn probe(conn: &Connection) -> Result<String> {
    let cfg = config(conn)?;
    if cfg.key_broken {
        bail!("密钥读不出来了，重新填一次");
    }
    if !cfg.ready() {
        bail!("还差一个 API 密钥");
    }
    let cands = vec![
        Candidate {
            id: 1,
            what: "读完一章书".to_string(),
        },
        Candidate {
            id: 2,
            what: "做完一章习题".to_string(),
        },
    ];
    let t0 = std::time::Instant::now();
    let pick = classify(&cfg, "把第 3 章的习题做完了", &cands)?;
    let ms = t0.elapsed().as_millis();
    let what = match pick.source_id {
        Some(1) => "读完一章书",
        Some(2) => "做完一章习题",
        // 说「不知道」是规矩允许的答案，不是失败——这一条也要讲清楚，
        // 否则第一次用的人会以为配坏了。
        _ => return Ok(format!("连接正常，用的模型是 {}，{} 毫秒。", cfg.model, ms)),
    };
    Ok(format!(
        "连接正常，用的模型是 {}，{} 毫秒。拿一条「把第 3 章的习题做完了」试它，它选了「{}」，把握 {}。",
        cfg.model, ms, what, pick.confidence
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-10-07";

    fn cands() -> Vec<Candidate> {
        vec![
            Candidate {
                id: 7,
                what: "读完一章".to_string(),
            },
            Candidate {
                id: 9,
                what: "做完一章题".to_string(),
            },
        ]
    }

    fn reply(content: &str) -> Value {
        json!({ "choices": [ { "message": { "content": content } } ] })
    }

    #[test]
    fn 挑中的规则必须在候选里() {
        let c = cands();
        let p = parse_reply(&reply(r#"{"id": 9, "confidence": 90}"#), &c).unwrap();
        assert_eq!(p.source_id, Some(9));
        assert_eq!(p.confidence, 90);
    }

    /// 模型编一个没给它的 id 出来 —— 这是最危险的一种回复：
    /// 归到一条不存在的（或者别人家的）规则上，曲线会动，而且没人看得出为什么。
    #[test]
    fn 候选之外的_id_一律作废() {
        let c = cands();
        for bad in [r#"{"id": 1, "confidence": 99}"#, r#"{"id": 0, "confidence": 99}"#] {
            let p = parse_reply(&reply(bad), &c).unwrap();
            assert_eq!(p.source_id, None, "不该接受候选外的 id：{bad}");
            assert_eq!(p.confidence, 0, "没挑中的时候置信度必须归零");
        }
    }

    #[test]
    fn 说不知道是合法回答() {
        let c = cands();
        let p = parse_reply(&reply(r#"{"id": null, "confidence": 20}"#), &c).unwrap();
        assert_eq!(p.source_id, None);
        assert_eq!(p.confidence, 0);
    }

    #[test]
    fn 代码块包着的_json_也认() {
        let c = cands();
        let p = parse_reply(
            &reply("```json\n{\"id\": 7, \"confidence\": 85}\n```"),
            &c,
        )
        .unwrap();
        assert_eq!(p.source_id, Some(7));
    }

    #[test]
    fn 前言后语包着的_json_也认() {
        let c = cands();
        let p = parse_reply(
            &reply("我觉得是这样：{\"id\": 7, \"confidence\": 88} 就这样。"),
            &c,
        )
        .unwrap();
        assert_eq!(p.source_id, Some(7));
    }

    #[test]
    fn 置信度会被夹到合法区间() {
        let c = cands();
        let p = parse_reply(&reply(r#"{"id": 7, "confidence": 555}"#), &c).unwrap();
        assert_eq!(p.confidence, 100);
        let p = parse_reply(&reply(r#"{"id": 7, "confidence": -3}"#), &c).unwrap();
        assert_eq!(p.confidence, 0);
        // 没写 confidence 就当没有把握。
        let p = parse_reply(&reply(r#"{"id": 7}"#), &c).unwrap();
        assert_eq!(p.confidence, 0);
    }

    #[test]
    fn 回一段大白话就是失败而不是归属() {
        let c = cands();
        // 没有 JSON：报错，让人自己选。**不能猜。**
        assert!(parse_reply(&reply("这条记录看起来像是做完了一章题"), &c).is_err());
        // 连 content 都没有。
        assert!(parse_reply(&json!({"error": "boom"}), &c).is_err());
    }

    // ------------------------------------------------------------ 密钥

    #[test]
    fn 密钥加密后能解回来并且不是明文() {
        let plain = "sk-abcdef1234567890";
        let blob = seal(plain).unwrap();
        assert!(!blob.contains("sk-"), "密文里不该出现明文：{blob}");
        assert!(!blob.contains("abcdef"), "密文里不该出现明文");
        assert_eq!(open(&blob).unwrap(), plain);
    }

    #[test]
    fn 同一个密钥两次加密结果不同() {
        // nonce 每次重新随机。否则两条密文的异或就把明文泄出去了。
        let a = seal("sk-same").unwrap();
        let b = seal("sk-same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn 密文被改过就解不开() {
        let blob = seal("sk-abcdef1234567890").unwrap();
        // 动最后一个十六进制字符
        let mut tampered = blob.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'a' { 'b' } else { 'a' });
        assert!(open(&tampered).is_err(), "改了密文还能解开，说明没有完整性校验");

        assert!(open("这不是十六进制").is_err());
        assert!(open("00").is_err());
    }

    #[test]
    fn 掩码不泄露密钥本身() {
        assert_eq!(mask(""), "");
        let m = mask("sk-abcdef1234567890");
        assert!(!m.contains("abcdef"), "掩码里不能有中间那段：{m}");
        assert!(m.ends_with("7890"), "留最后四位是为了让人认出是哪一把");
        assert_eq!(mask("abc"), "••••");
    }

    // ------------------------------------------------------------ 提示词

    #[test]
    fn 提示词里带着候选的_id_和原话() {
        let cfg = Config {
            enabled: true,
            base_url: "https://example.invalid/v1".to_string(),
            model: "m".to_string(),
            api_key: "k".to_string(),
            threshold: 80,
            key_broken: false,
        };
        let req = build_request(&cfg, "做完了第 3 章习题", &cands());
        let user = req["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("id = 7"), "{user}");
        assert!(user.contains("做完一章题"), "{user}");
        assert!(user.contains("做完了第 3 章习题"), "{user}");
        // 温度固定 0：同一个输入要尽量给同一个答案。
        assert_eq!(req["temperature"], 0);
    }

    #[test]
    fn 设置能存能读且密钥不明文落库() {
        let conn = db::open_memory().unwrap();
        save(
            &conn,
            true,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            85,
            Some("sk-secret-1234"),
        )
        .unwrap();

        // 落库的那一份必须是密文。
        let raw = db::setting_get(&conn, K_KEY).unwrap().unwrap();
        assert!(!raw.contains("sk-secret"), "库里存了明文：{raw}");

        let cfg = config(&conn).unwrap();
        assert!(cfg.ready());
        assert_eq!(cfg.api_key, "sk-secret-1234");
        assert_eq!(cfg.threshold, 85);
        assert_eq!(cfg.model, "deepseek-chat");
        assert!(!cfg.key_broken);

        // None = 不动原来那把。
        save(&conn, false, "", "", 50, None).unwrap();
        assert_eq!(config(&conn).unwrap().api_key, "sk-secret-1234");
        assert!(!config(&conn).unwrap().enabled);

        // Some("") = 清掉。
        save(&conn, true, DEFAULT_BASE_URL, DEFAULT_MODEL, 80, Some("  ")).unwrap();
        assert!(config(&conn).unwrap().api_key.is_empty());
        assert!(!config(&conn).unwrap().ready());
    }

    #[test]
    fn 没配密钥时什么都不做也不算错() {
        let conn = db::open_memory().unwrap();
        let (g, _) = two_rule_goal(&conn);
        let id = loose_record(&conn, &[g], "读了点东西");

        // 没有密钥 → 不发请求、不猜。记录照旧挂空着，等人在详情页指认。
        let f = classify_one(&conn, id, d(TODAY)).unwrap();
        assert_eq!(f.decided, 0);
        assert!(f.trouble.is_none(), "没配 AI 不算「出问题」");
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 1, "还是那条待办");
    }

    #[test]
    fn 全库没有手工规则时不发请求() {
        // 一个候选都没有，模型没有可挑的东西。靠不通的网络地址来证明它真的没发请求。
        let conn = db::open_memory().unwrap();
        db::goal_add(&conn, "英语", "", "blue", TODAY).unwrap();
        let id = db::checkin_add(&conn, &[], TODAY, "10:00", 1.0, "随便记一句", TODAY).unwrap();
        save(&conn, true, "http://127.0.0.1:1/v1", "m", 80, Some("k")).unwrap();

        let f = classify_one(&conn, id, d(TODAY)).unwrap();
        assert_eq!(f.decided, 0);
        assert!(f.trouble.is_none(), "没有候选就不该发请求");
    }

    /// 没关联目标的记录，哪怕全库只有一条手工规则，**也要问模型**。
    ///
    /// 这和「挂了目标、目标下只有一条规则」不是一回事：那时候规则是唯一确定的；
    /// 这里要判的是「这件事到底算不算那条规则」——写代码不该推动「英语」那条线，
    /// 而候选只有一个的时候，只有模型能回答「都不是」。
    #[test]
    fn 没关联目标时哪怕只有一条候选也要问() {
        let conn = db::open_memory().unwrap();
        let g = db::goal_add(&conn, "英语", "", "blue", TODAY).unwrap();
        db::source_add(
            &conn,
            g,
            crate::model::SourceKind::ManualCheckin,
            "复习",
            "{}",
            "复习一轮算一次",
            TODAY,
        )
        .unwrap();
        let id = db::checkin_add(&conn, &[], TODAY, "10:00", 1.0, "写了点代码", TODAY).unwrap();
        // 指向连不上的地址：真发了请求就会留下 trouble，正好用来证明它发了。
        save(&conn, true, "http://127.0.0.1:1/v1", "m", 80, Some("k")).unwrap();

        let f = classify_one(&conn, id, d(TODAY)).unwrap();
        assert_eq!(f.decided, 0);
        assert!(f.trouble.is_some(), "这一条必须问模型，不该被本地规则挡掉");
    }

    #[test]
    fn 补判只填挂空的那一格() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = two_rule_goal(&conn);
        // 人挑过的那条。
        let links = db::resolve_links(&conn, &[g], &[(g, s1)], false).unwrap();
        let id = db::checkin_add(&conn, &links, TODAY, "10:00", 1.0, "复习第 3 课", TODAY).unwrap();

        // 指向一个连不上的地址：真去补判会失败，于是「有没有动过原来那条」才验得出来。
        save(&conn, true, "http://127.0.0.1:1/v1", "m", 80, Some("k")).unwrap();
        let _ = classify_one(&conn, id, d(TODAY)).unwrap();

        // 人挑的那条不能被改写。
        assert_eq!(db::source_checkin_value(&conn, s1, TODAY).unwrap(), 1.0);
        assert!(
            db::unattributed_of(&conn, g).unwrap().is_empty(),
            "本来就没人挂空"
        );
    }

    /// 一条「挂了目标但没归到规则」的记录 —— 说不清时 `defer = true` 的产物。
    fn loose_record(conn: &Connection, goals: &[i64], note: &str) -> i64 {
        let links = db::resolve_links(conn, goals, &[], true).unwrap();
        db::checkin_add(conn, &links, TODAY, "10:00", 1.0, note, TODAY).unwrap()
    }

    fn two_rule_goal(conn: &Connection) -> (i64, i64) {
        let g = db::goal_add(conn, "计算机基础", "", "blue", TODAY).unwrap();
        let s1 = db::source_add(
            conn,
            g,
            crate::model::SourceKind::ManualCheckin,
            "读完一章",
            "{}",
            "读完一章",
            TODAY,
        )
        .unwrap();
        db::source_add(
            conn,
            g,
            crate::model::SourceKind::ManualCheckin,
            "做完一章题",
            "{}",
            "做完一章题",
            TODAY,
        )
        .unwrap();
        (g, s1)
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
}
