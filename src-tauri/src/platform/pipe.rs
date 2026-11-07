//! 回填推送通道的公共定义。

/// 管道名（GUI 作为客户端连它）。
pub const PIPE_NAME: &str = r"\\.\pipe\lemma-writeback";

/// 宿主对一次推送的结果。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
        }
    }
}
