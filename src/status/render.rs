//! 照取出来的四样写结果：md 的中文、json 的那个样子，一行行照原来脚本的 `main()`、`to_json()` 写，Python 的规矩见 [`super::py`]。

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::URL;
use super::py::{self, Moment, get, key, or, str_of};

/// 状态的严重程度：同一个服务被几起故障影响时取最重的。
fn severity(state: Option<&Value>) -> u8 {
    match state.and_then(Value::as_str) {
        Some("full_outage" | "major_outage") => 4,
        Some("partial_outage") => 3,
        Some("degraded" | "degraded_performance") => 2,
        Some("maintenance") => 1,
        _ => 0,
    }
}

/// 状态的中文；不认识的照原样，空的写「未知」。
fn zh(state: &Value) -> String {
    let named = match state.as_str() {
        Some("operational") => "正常",
        Some("full_outage" | "major_outage") => "完全中断",
        Some("partial_outage") => "部分中断",
        Some("degraded" | "degraded_performance") => "性能下降",
        Some("maintenance") => "维护中",
        Some("investigating") => "排查中",
        Some("identified") => "已定位",
        Some("monitoring") => "观察中",
        Some("resolved") => "已解决",
        _ if py::truthy(state) => return str_of(state),
        _ => "未知",
    };
    named.to_string()
}

/// 一个值是列表的，取里面的字典；别的（Python 里照样能迭代、但没有字典）是空的。
fn dicts(value: &Value) -> Vec<&Value> {
    value
        .as_array()
        .map(|items| items.iter().filter(|item| item.is_object()).collect())
        .unwrap_or_default()
}

/// `when()`：是数（不是真假）的当时间戳。
fn when(value: &Value) -> Option<Moment> {
    match value {
        Value::Number(number) => number.as_f64().map(Moment::from_timestamp),
        _ => None,
    }
}

/// `span()`：多少分钟、几小时几分钟。
fn span(seconds: i64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "故障时长的秒数远小于 2^52")]
    let float = |n: i64| n as f64;
    if seconds < 3600 {
        return format!("{} 分钟", py::round(float(seconds) / 60.0).max(1));
    }
    let (hours, rest) = (seconds.div_euclid(3600), seconds.rem_euclid(3600));
    if rest >= 60 {
        format!("{hours} 小时 {} 分钟", py::round(float(rest) / 60.0))
    } else {
        format!("{hours} 小时")
    }
}

/// 受影响的服务名：`name`，没有用 `component_name`，都没有写 `?`。
fn names(change: &Value) -> Vec<Value> {
    let question = json!("?");
    dicts(get(change, "affected_components"))
        .into_iter()
        .map(|item| {
            or(
                or(get(item, "name"), get(item, "component_name")),
                &question,
            )
            .clone()
        })
        .collect()
}

fn joined(names: &[Value]) -> String {
    names.iter().map(str_of).collect::<Vec<_>>().join("、")
}

/// 最新的一条进展（时刻一样的取前面那条）：只要中文那段（中文在前、英文在后，空一行分开）。
fn last_update(change: &Value) -> String {
    let updates = dicts(get(change, "updates"));
    let mut latest: Option<&Value> = None;
    for update in updates {
        let at = py::number(or(get(update, "at_seconds"), &json!(0)));
        if latest.is_none_or(|best| at > py::number(or(get(best, "at_seconds"), &json!(0)))) {
            latest = Some(update);
        }
    }
    let Some(latest) = latest else {
        return String::new();
    };
    let text = py::strip(get(latest, "description").as_str().unwrap_or_default());
    py::strip(text.split("\n\n").next().unwrap_or_default()).to_string()
}

/// 有这一格的取它，没有的写 `?`（Python 的 `.get(key, '?')`：有这一格、是 `null` 的照样印 `None`）。
fn or_question(value: &Value, field: &str) -> String {
    value.get(field).map_or_else(|| "?".to_string(), str_of)
}

/// 从页面取出来、算好的：服务列表、各服务的可用率和状态、正在处理的、最近的（已经截到要的条数）。
struct View<'a> {
    components: Vec<&'a Value>,
    uptime: HashMap<String, Value>,
    state: HashMap<String, Value>,
    active: Vec<&'a Value>,
    history: Vec<&'a Value>,
}

impl View<'_> {
    /// 可用率：是数（真假也算，Python 里 bool 是 int）的才有。
    fn uptime_of(&self, component: &Value) -> Option<Value> {
        let id = key(get(component, "component_id"))?;
        self.uptime
            .get(&id)
            .filter(|rate| rate.is_number() || rate.is_boolean())
            .cloned()
    }

    /// 状态：没被正在处理的故障影响的是 `operational`。
    fn state_of(&self, component: &Value) -> Value {
        key(get(component, "component_id"))
            .and_then(|id| self.state.get(&id).cloned())
            .unwrap_or_else(|| json!("operational"))
    }
}

/// 一页的结果：md 的或 json 的一整段（不带末尾的换行）。
///
/// # Errors
///
/// 读不出服务列表：页面的结构变了。
pub fn page(
    found: &HashMap<&'static str, Value>,
    count: usize,
    now: Moment,
    as_json: bool,
) -> Result<String, String> {
    let null = Value::Null;
    let empty = Value::Object(Map::new());
    let found_get = |name: &str| found.get(name).unwrap_or(&null);
    let components = dicts(get(
        or(found_get("initialPageConfig"), &empty),
        "components",
    ));
    if components.is_empty() {
        return Err(format!(
            "状态页的结构变了，读不出服务列表。可以让用户自己打开 {URL} 看"
        ));
    }
    let mut uptime = HashMap::new();
    for item in dicts(found_get("component_uptimes")) {
        if let Some(id) = key(get(item, "component_id")) {
            uptime.insert(id, get(item, "uptime").clone());
        }
    }
    let active = dicts(found_get("active_changes"));
    let mut state: HashMap<String, Value> = HashMap::new();
    let unknown = json!("unknown");
    for change in &active {
        for item in dicts(get(change, "affected_components")) {
            let Some(id) = key(get(item, "component_id")) else {
                continue;
            };
            let status = or(get(change, "status"), &unknown);
            if severity(Some(status)) >= severity(state.get(&id)) {
                state.insert(id, status.clone());
            }
        }
    }
    let mut history = dicts(get(or(found_get("initialCalendarData"), &empty), "changes"));
    // 新的在前；一样的照原来的先后（稳定排序，和 Python 的 sort(reverse=True) 一样）。
    history.sort_by(|a, b| {
        let start = |change: &Value| py::number(or(get(change, "start_at_seconds"), &json!(0)));
        start(b).total_cmp(&start(a))
    });
    history.truncate(count);
    let view = View {
        components,
        uptime,
        state,
        active,
        history,
    };
    Ok(if as_json {
        json_page(&view, now)
    } else {
        md_page(&view, count, now)
    })
}

/// 原来脚本的 `to_json()`。
fn json_page(view: &View<'_>, now: Moment) -> String {
    let change_json = |change: &Value, ongoing: bool| -> Value {
        let mut out = json!({
            "title": get(change, "title"), "type": get(change, "type"), "status": get(change, "status"),
            "started": when(get(change, "start_at_seconds")).map(Moment::iso_minute),
            "affected": names(change),
        });
        if ongoing {
            let latest = last_update(change);
            out["latest_update"] = if latest.is_empty() {
                Value::Null
            } else {
                json!(latest)
            };
        } else {
            out["ended"] = json!(when(get(change, "close_at_seconds")).map(Moment::iso_minute));
        }
        out
    };
    let components: Vec<Value> = view
        .components
        .iter()
        .map(|component| {
            json!({"name": get(component, "name"), "status": view.state_of(component),
                   "uptime_30d": view.uptime_of(component)})
        })
        .collect();
    py::dumps(&json!({
        "ok": true, "checked": now.iso_minute(), "source": URL, "all_operational": view.active.is_empty(),
        "components": components,
        "ongoing": view.active.iter().map(|change| change_json(change, true)).collect::<Vec<_>>(),
        "recent": view.history.iter().map(|change| change_json(change, false)).collect::<Vec<_>>(),
    }))
}

/// 原来脚本 `main()` 里写 md 的那一段。
fn md_page(view: &View<'_>, count: usize, now: Moment) -> String {
    let mut out = vec![format!(
        "DeepSeek 服务状态（{} 北京时间，来源 {URL}）",
        now.date_minute()
    )];
    out.push(if view.active.is_empty() {
        "全部正常".to_string()
    } else {
        format!("有 {} 起故障正在处理", view.active.len())
    });
    out.push(String::new());
    for component in &view.components {
        let rate = view.uptime_of(component).map_or_else(String::new, |rate| {
            format!("，30 天可用率 {}%", str_of(&rate))
        });
        out.push(format!(
            "- {}：{}{rate}",
            or_question(component, "name"),
            zh(&view.state_of(component))
        ));
    }
    if !view.active.is_empty() {
        out.push(String::new());
        out.push("正在处理：".to_string());
        for change in &view.active {
            let began = when(get(change, "start_at_seconds")).map_or_else(String::new, |start| {
                format!("{} 起，", start.month_minute())
            });
            out.push(format!(
                "- {}（{began}{}）",
                or_question(change, "title"),
                zh(get(change, "status"))
            ));
            push_affected(&mut out, change);
            let latest = last_update(change);
            if !latest.is_empty() {
                out.push(format!("  最新：{latest}"));
            }
        }
    }
    if count > 0 && !view.history.is_empty() {
        out.push(String::new());
        out.push(format!("最近 {} 起故障：", view.history.len()));
        for change in &view.history {
            let start = when(get(change, "start_at_seconds"));
            let end = when(get(change, "close_at_seconds"));
            let period = match (start, end) {
                (Some(start), Some(end)) => format!(
                    "{}，持续 {}",
                    start.month_minute(),
                    span(start.seconds_until(end))
                ),
                (Some(start), None) => format!("{} 起，还没结束", start.month_minute()),
                _ => "时间不明".to_string(),
            };
            let kind = if get(change, "type").as_str() == Some("maintenance") {
                "维护".to_string()
            } else {
                zh(get(change, "status"))
            };
            out.push(format!(
                "- {}（{period}，{kind}）",
                or_question(change, "title")
            ));
            push_affected(&mut out, change);
        }
    }
    out.join("\n")
}

/// 有受影响的服务的，加一行「影响：…」。
fn push_affected(out: &mut Vec<String>, change: &Value) {
    let affected = names(change);
    if !affected.is_empty() {
        out.push(format!("  影响：{}", joined(&affected)));
    }
}
