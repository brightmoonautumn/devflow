use super::{LIMIT, Result, SseOptions, error};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) fn read(
    request: reqwest::blocking::Request,
    options: &SseOptions,
    budget: Duration,
    cancel: Option<&AtomicBool>,
    mut on_event: impl FnMut(u16, Value) -> Result<()>,
) -> Result<(u16, Value)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| error("unavailable", "无法启动 SSE 读取器"))?;
    runtime.block_on(async move {
        let read = async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .build()
                .map_err(|_| error("unavailable", "无法创建 SSE 执行器"))?;
            let mut builder = client.request(request.method().clone(), request.url().clone())
                .headers(request.headers().clone());
            if let Some(body) = request.body() {
                let bytes = body.as_bytes().ok_or_else(|| error("invalid_request", "SSE 请求体必须可序列化"))?;
                builder = builder.body(bytes.to_vec());
            }
            let mut response = builder.send().await.map_err(|_| error("unavailable", "无法连接 SSE 接口"))?;
            let status = response.status().as_u16();
            if status != 200 {
                return Err(error("assertion_failed", format!("SSE 接口返回 HTTP {status}")));
            }
            let media_type = response.headers().get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("");
            if !media_type.trim().eq_ignore_ascii_case("text/event-stream") {
                return Err(error("invalid_request", "接口响应不是 text/event-stream"));
            }
            let mut parser = Parser::default();
            let mut total = 0;
            while let Some(chunk) = response.chunk().await.map_err(|_| error("unavailable", "读取 SSE 失败"))? {
                total += chunk.len() as u64;
                if total > LIMIT {
                    return Err(error("invalid_request", "SSE 响应超过 1 MiB 上限"));
                }
                for &byte in chunk.as_ref() {
                    let Some((event, data)) = parser.push(byte)? else { continue };
                    if event == options.end_event {
                        let value = serde_json::from_str(&data)
                            .map_err(|_| error("invalid_request", "SSE 结束事件的 data 不是有效 JSON"))?;
                        return Ok((status, value));
                    }
                    let data = serde_json::from_str(&data).unwrap_or(Value::String(data));
                    on_event(status, json!({ "event": event, "data": data }))?;
                }
            }
            Err(error("assertion_failed", format!("SSE 已关闭，未收到结束事件 {}", options.end_event)))
        };
        let cancelled = async {
            let Some(cancel) = cancel else { return std::future::pending::<()>().await };
            while !cancel.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        tokio::select! {
            biased;
            _ = cancelled => Err(error("cancelled", "已取消 SSE 读取")),
            result = tokio::time::timeout(budget, read) => result
                .map_err(|_| error("unavailable", "SSE 等待结束事件超时"))?,
        }
    })
}

#[derive(Default)]
struct Parser {
    line: Vec<u8>,
    event: String,
    data: Vec<String>,
    after_cr: bool,
    seen_line: bool,
}

impl Parser {
    fn push(&mut self, byte: u8) -> Result<Option<(String, String)>> {
        let skip = self.after_cr && byte == b'\n';
        self.after_cr = byte == b'\r';
        if skip {
            return Ok(None);
        }
        if !matches!(byte, b'\r' | b'\n') {
            self.line.push(byte);
            return Ok(None);
        }

        let line = std::str::from_utf8(&self.line)
            .map_err(|_| error("invalid_request", "SSE 事件不是 UTF-8"))?;
        let line = if self.seen_line { line } else { line.trim_start_matches('\u{feff}') };
        self.seen_line = true;
        let result = if line.is_empty() {
            let event = std::mem::take(&mut self.event);
            if self.data.is_empty() {
                None
            } else {
                let name = if event.is_empty() { "message".into() } else { event };
                let data = self.data.join("\n");
                self.data.clear();
                Some((name, data))
            }
        } else {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => self.event = value.into(),
                "data" => self.data.push(value.into()),
                _ => {}
            }
            None
        };
        self.line.clear();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_multiline_events_and_ignores_heartbeats() {
        let input = "\u{feff}: heartbeat\r\n\r\nevent: run.progress\rdata: 中文\r\r\nevent: run.completed\ndata: {\ndata: \"ok\":true}\n\n";
        let mut parser = Parser::default();
        let events: Vec<_> = input.bytes().filter_map(|byte| parser.push(byte).unwrap()).collect();
        assert_eq!(events, vec![
            ("run.progress".to_owned(), "中文".to_owned()),
            ("run.completed".to_owned(), "{\n\"ok\":true}".to_owned()),
        ]);
    }
}
