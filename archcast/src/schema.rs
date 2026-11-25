use serde::{Deserialize, Serialize};

pub const SPEC_VERSION: u32 = 1;

/// Semantic palette matching the canonical overview visual specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Default,
    Muted,
    Blue,
    Orange,
    Green,
    Emerald,
    Amber,
    Purple,
    Gold,
    Red,
}

impl Tone {
    /// Hex color for accent bars, pills, and highlighted text.
    pub fn hex(self) -> &'static str {
        match self {
            Tone::Default => "#334155",
            Tone::Muted => "#8b95a5",
            Tone::Blue => "#4b83d4",
            Tone::Orange => "#e08b3c",
            Tone::Green => "#3e9e6e",
            Tone::Emerald => "#0f8a5f",
            Tone::Amber => "#b45309",
            Tone::Purple => "#7b61c9",
            Tone::Gold => "#c9a227",
            Tone::Red => "#d9534f",
        }
    }

    /// Map a hex string observed in legacy HTML/SVG back to the nearest canonical Tone.
    pub fn from_hex(hex: &str) -> Self {
        let lower = hex.trim().to_ascii_lowercase();
        match lower.as_str() {
            "#4b83d4" => Tone::Blue,
            "#e08b3c" => Tone::Orange,
            "#3e9e6e" => Tone::Green,
            "#0f8a5f" => Tone::Emerald,
            "#b45309" | "#7a4a12" | "#7c5a12" => Tone::Amber,
            "#7b61c9" => Tone::Purple,
            "#c9a227" => Tone::Gold,
            "#d9534f" => Tone::Red,
            "#8b95a5" | "#94a3b8" | "#9aa4b2" | "#64748b" | "#6b7280" => Tone::Muted,
            _ => Tone::Default,
        }
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Tone::Default)
    }
}

/// Top-level single-source-of-truth specification for an architecture / flow overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchSpec {
    #[serde(default = "default_spec_version")]
    pub schema_version: u32,
    /// HTML `<title>` text. Defaults to `title` if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_title: Option<String>,
    /// Main `<h1>` heading.
    pub title: String,
    /// Lead summary paragraphs rendered under TOC (plain text; inline `code` via backticks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lead: Vec<String>,
    /// Ordered architectural diagrams (e.g., Current State + Target State).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<DiagramSpec>,
    /// Optional structured API / callback flow table (Section 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_table: Option<FlowTableSpec>,
}

fn default_spec_version() -> u32 {
    SPEC_VERSION
}

/// One full swimlane diagram card (`<div class="card">` + inline `<svg>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramSpec {
    /// HTML anchor ID (e.g. `fig1`, `fig2`).
    pub id: String,
    /// Card `<h2>` heading (e.g. `一、链路全景（现状）`).
    pub heading: String,
    /// Short note above the SVG.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// SVG `<title>` element text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub svg_title: String,
    /// Main title rendered inside the SVG canvas.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub canvas_title: String,
    /// Secondary descriptive lines under the SVG canvas title.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitles: Vec<StyledLine>,
    /// Swimlane columns (canonical layout uses 4 columns: Trigger, Core, External, Read).
    #[serde(default)]
    pub columns: Vec<ColumnSpec>,
    /// Explicit semantic edges between cards/columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeSpec>,
    /// Bottom 2-column callout cards (e.g. request payload box + gap/summary box).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callouts: Vec<CalloutSpec>,
    /// Optional full-width warning banner at the bottom of the SVG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<WarningBannerSpec>,
    /// Footer citation line (e.g. `依据：...`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub footer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub id: String,
    pub title: String,
    /// Optional explicit width in SVG px; defaults to canonical 4-column widths (340/440/600/420).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default)]
    pub cards: Vec<CardSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSpec {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub accent: Tone,
    /// Optional top-right pill badge on the card header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<BadgeSpec>,
    /// Optional explicit Y position hint (e.g. to align read-side cards with external systems).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_with: Option<String>,
    /// Text/code lines rendered inside the card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<StyledLine>,
    /// Nested action subcards (used in external-system cards).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subcards: Vec<SubcardSpec>,
    /// Optional muted footer line at the bottom of the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubcardSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<BadgeSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<StyledLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BadgeSpec {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyledLine {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "Tone::is_default")]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

impl StyledLine {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            mono: false,
            bold: false,
            tone: Tone::Default,
            size: None,
        }
    }

    pub fn mono(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            mono: true,
            bold: false,
            tone: Tone::Default,
            size: None,
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Direct arrow between one source card and one target card.
    #[default]
    Direct,
    /// Shared vertical bus collecting one or more sources into one or more targets.
    Bus,
    /// Left-side bypass channel connecting two cards in the same column.
    LoopLeft,
    /// Routed cleanly along a horizontal channel below the main columns.
    BottomRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    #[serde(default)]
    pub kind: EdgeKind,
    /// Source card IDs (1 for Direct/LoopLeft/BottomRoute; 1+ for Bus).
    pub from: Vec<String>,
    /// Target card IDs (1 for Direct/LoopLeft/BottomRoute; 1+ for Bus).
    pub to: Vec<String>,
    /// Optional label rendered near the bus or arrow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional bus X offset ratio (0.0..1.0 between source right & target left) for separating parallel buses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus_slot: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalloutStyle {
    #[default]
    Bullets,
    CodeBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalloutSpec {
    pub title: String,
    #[serde(default)]
    pub accent: Tone,
    #[serde(default)]
    pub style: CalloutStyle,
    #[serde(default)]
    pub lines: Vec<StyledLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarningBannerSpec {
    pub title: String,
    pub body: String,
}

/// Section 3: Grouped API / workflow / callback matrix table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowTableSpec {
    #[serde(default = "default_flow_table_id")]
    pub id: String,
    pub heading: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub groups: Vec<FlowGroupSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

fn default_flow_table_id() -> String {
    "flowtable".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowGroupSpec {
    pub name: String,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default)]
    pub rows: Vec<FlowRowSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowRowSpec {
    pub action: String,
    pub trigger: String,
    pub version: String,
    #[serde(default)]
    pub version_tone: Tone,
    pub entry: Vec<String>,
    pub downstream: Vec<String>,
    #[serde(default)]
    pub note: String,
}

/// Generate a minimal starter spec demonstrating the canonical 4-column before/after structure.
pub fn starter_template(title: &str) -> ArchSpec {
    ArchSpec {
        schema_version: SPEC_VERSION,
        doc_title: Some(title.to_string()),
        title: title.to_string(),
        lead: vec![
            "一句话：触发源发起 → 核心编排落库 → 调外部系统 → 读侧消费。".to_string(),
            "本次改造仅新增 `api_version` 字段并在下游透传，已有字段保持兼容。".to_string(),
        ],
        diagrams: vec![
            DiagramSpec {
                id: "fig1".to_string(),
                heading: "一、链路全景（现状）".to_string(),
                note: "左：触发源；中：核心编排；右：外部系统；最右：读侧。".to_string(),
                svg_title: format!("{title}（现状）"),
                canvas_title: format!("{title} · 现状"),
                subtitles: vec![StyledLine {
                    text: "红标 = 当前缺失版本标识".to_string(),
                    mono: false,
                    bold: false,
                    tone: Tone::Muted,
                    size: Some(13.5),
                }],
                columns: vec![
                    ColumnSpec {
                        id: "triggers".to_string(),
                        title: "① 触发源".to_string(),
                        width: None,
                        cards: vec![CardSpec {
                            id: "ui_create".to_string(),
                            title: "人 · 页面发起".to_string(),
                            accent: Tone::Blue,
                            badge: None,
                            align_with: None,
                            lines: vec![
                                StyledLine::plain("弹窗表单提交"),
                                StyledLine::mono("POST /api/v1/create"),
                                StyledLine {
                                    text: "api_version：缺失".to_string(),
                                    mono: true,
                                    bold: true,
                                    tone: Tone::Red,
                                    size: None,
                                },
                            ],
                            subcards: vec![],
                            footer: None,
                        }],
                    },
                    ColumnSpec {
                        id: "core".to_string(),
                        title: "② 核心编排 / 存储".to_string(),
                        width: None,
                        cards: vec![
                            CardSpec {
                                id: "core_api".to_string(),
                                title: "入口 API".to_string(),
                                accent: Tone::Green,
                                badge: None,
                                align_with: None,
                                lines: vec![StyledLine::mono("POST /flow/create")],
                                subcards: vec![],
                                footer: None,
                            },
                            CardSpec {
                                id: "core_store".to_string(),
                                title: "主流程表".to_string(),
                                accent: Tone::Green,
                                badge: None,
                                align_with: None,
                                lines: vec![
                                    StyledLine {
                                        text: "wf_main_record".to_string(),
                                        mono: true,
                                        bold: false,
                                        tone: Tone::Muted,
                                        size: None,
                                    },
                                    StyledLine {
                                        text: "✗ 无 api_version 字段".to_string(),
                                        mono: false,
                                        bold: false,
                                        tone: Tone::Red,
                                        size: None,
                                    },
                                ],
                                subcards: vec![],
                                footer: None,
                            },
                        ],
                    },
                    ColumnSpec {
                        id: "external".to_string(),
                        title: "③ 外部系统".to_string(),
                        width: None,
                        cards: vec![CardSpec {
                            id: "ext_service".to_string(),
                            title: "下游服务".to_string(),
                            accent: Tone::Purple,
                            badge: Some(BadgeSpec {
                                text: "缺版本".to_string(),
                                tone: Tone::Red,
                            }),
                            align_with: None,
                            lines: vec![],
                            subcards: vec![SubcardSpec {
                                title: "创建任务".to_string(),
                                badge: Some(BadgeSpec {
                                    text: "✗ 只有 v1".to_string(),
                                    tone: Tone::Red,
                                }),
                                lines: vec![StyledLine::mono("POST /ext/v1/task")],
                            }],
                            footer: Some("主流程调用".to_string()),
                        }],
                    },
                    ColumnSpec {
                        id: "readers".to_string(),
                        title: "④ 读侧".to_string(),
                        width: None,
                        cards: vec![CardSpec {
                            id: "read_hub".to_string(),
                            title: "结果总览页".to_string(),
                            accent: Tone::Gold,
                            badge: None,
                            align_with: Some("ext_service".to_string()),
                            lines: vec![
                                StyledLine::mono("GET /ext/v1/list"),
                                StyledLine {
                                    text: "✗ 无法按版本筛选".to_string(),
                                    mono: false,
                                    bold: false,
                                    tone: Tone::Red,
                                    size: None,
                                },
                            ],
                            subcards: vec![],
                            footer: None,
                        }],
                    },
                ],
                edges: vec![
                    EdgeSpec {
                        kind: EdgeKind::Bus,
                        from: vec!["ui_create".to_string()],
                        to: vec!["core_api".to_string()],
                        label: Some("触发".to_string()),
                        bus_slot: None,
                    },
                    EdgeSpec {
                        kind: EdgeKind::Direct,
                        from: vec!["core_api".to_string()],
                        to: vec!["core_store".to_string()],
                        label: None,
                        bus_slot: None,
                    },
                    EdgeSpec {
                        kind: EdgeKind::Bus,
                        from: vec!["core_store".to_string()],
                        to: vec!["ext_service".to_string()],
                        label: Some("主流程调用".to_string()),
                        bus_slot: Some(0.25),
                    },
                    EdgeSpec {
                        kind: EdgeKind::Direct,
                        from: vec!["ext_service".to_string()],
                        to: vec!["read_hub".to_string()],
                        label: None,
                        bus_slot: None,
                    },
                ],
                callouts: vec![
                    CalloutSpec {
                        title: "下游请求创参".to_string(),
                        accent: Tone::Purple,
                        style: CalloutStyle::CodeBox,
                        lines: vec![
                            StyledLine::mono("task_id, project_id"),
                            StyledLine {
                                text: "api_version ——（待新增）".to_string(),
                                mono: true,
                                bold: false,
                                tone: Tone::Red,
                                size: None,
                            },
                        ],
                    },
                    CalloutSpec {
                        title: "当前缺口".to_string(),
                        accent: Tone::Red,
                        style: CalloutStyle::Bullets,
                        lines: vec![
                            StyledLine::plain("① 入口无版本可选，主表未落库"),
                            StyledLine::plain("② 下游调用与读侧均无法区分 v1/v2"),
                        ],
                    },
                ],
                warning: Some(WarningBannerSpec {
                    title: "⚠ 改造注意事项".to_string(),
                    body: "存量数据默认按 v1 兼容处理，所有异步回调需核对版本一致性。".to_string(),
                }),
                footer: "依据：service/api/*.proto · service/internal/client/*.go".to_string(),
            },
        ],
        flow_table: Some(FlowTableSpec {
            id: "flowtable".to_string(),
            heading: "二、入口 → 核心编排 → 下游服务".to_string(),
            note: "每行对应一个入口动作及其编排入口与下游调用。".to_string(),
            groups: vec![FlowGroupSpec {
                name: "主流程".to_string(),
                tone: Tone::Blue,
                rows: vec![FlowRowSpec {
                    action: "发起创建".to_string(),
                    trigger: "前端".to_string(),
                    version: "v1".to_string(),
                    version_tone: Tone::Blue,
                    entry: vec!["POST /flow/create".to_string()],
                    downstream: vec!["POST /ext/v1/task".to_string()],
                    note: "基础创建链路".to_string(),
                }],
            }],
            hint: Some("新增 v2 路径时同步补齐对应回调接口。".to_string()),
        }),
    }
}
