//! Canonical Markdown + Mermaid documentation emitter for AI context and engineering wikis.

use crate::schema::{ArchSpec, CalloutStyle, DiagramSpec, EdgeKind, FlowTableSpec};
use std::fmt::Write;

/// Render an [`ArchSpec`] into a concise, token-efficient, AI- & Wiki-friendly Markdown document.
pub fn render_markdown_document(spec: &ArchSpec) -> String {
    let mut out = String::with_capacity(8_192);
    let _ = writeln!(out, "# {}\n", spec.title);

    if !spec.lead.is_empty() {
        for line in &spec.lead {
            let _ = writeln!(out, "> {line}");
        }
        out.push('\n');
    }

    for diag in &spec.diagrams {
        render_diagram_md(diag, &mut out);
    }

    if let Some(ft) = &spec.flow_table {
        render_flow_table_md(ft, &mut out);
    }

    out
}

fn render_diagram_md(diag: &DiagramSpec, out: &mut String) {
    let _ = writeln!(out, "## {}\n", diag.heading);
    if !diag.note.is_empty() {
        let _ = writeln!(out, "{}\n", diag.note);
    }
    for sub in &diag.subtitles {
        let _ = writeln!(out, "- {}", sub.text);
    }
    if !diag.subtitles.is_empty() {
        out.push('\n');
    }

    // Emit explicit Mermaid topology graph (AI-friendly semantic connectivity).
    out.push_str("```mermaid\nflowchart LR\n");
    for col in &diag.columns {
        let col_id = mermaid_id(&col.id);
        let _ = writeln!(out, "  subgraph {}[\"{}\"]", col_id, escape_mermaid(&col.title));
        for card in &col.cards {
            let cid = mermaid_id(&card.id);
            let label = match &card.badge {
                Some(b) => format!("{} [{}]", card.title, b.text),
                None => card.title.clone(),
            };
            let _ = writeln!(out, "    {}[\"{}\"]", cid, escape_mermaid(&label));
        }
        out.push_str("  end\n");
    }

    for edge in &diag.edges {
        for src in &edge.from {
            for dst in &edge.to {
                let s = mermaid_id(src);
                let t = mermaid_id(dst);
                match (&edge.kind, &edge.label) {
                    (_, Some(lbl)) => {
                        let _ = writeln!(out, "  {s} -->|\"{}\"| {t}", escape_mermaid(lbl));
                    }
                    (EdgeKind::BottomRoute | EdgeKind::LoopLeft, None) => {
                        let _ = writeln!(out, "  {s} -.-> {t}");
                    }
                    (_, None) => {
                        let _ = writeln!(out, "  {s} --> {t}");
                    }
                }
            }
        }
    }
    out.push_str("```\n\n");

    // Structured component & action details.
    for col in &diag.columns {
        let _ = writeln!(out, "### {}\n", col.title);
        for card in &col.cards {
            match &card.badge {
                Some(b) => {
                    let _ = writeln!(out, "- **{}** (`{}`) — `[{}]`", card.title, card.id, b.text);
                }
                None => {
                    let _ = writeln!(out, "- **{}** (`{}`)", card.title, card.id);
                }
            }
            for line in &card.lines {
                if line.mono {
                    let _ = writeln!(out, "  - `{}`", line.text);
                } else {
                    let _ = writeln!(out, "  - {}", line.text);
                }
            }
            for sub in &card.subcards {
                match &sub.badge {
                    Some(b) => {
                        let _ = writeln!(out, "  - **{}** `[{}]`", sub.title, b.text);
                    }
                    None => {
                        let _ = writeln!(out, "  - **{}**", sub.title);
                    }
                }
                for sl in &sub.lines {
                    if sl.mono {
                        let _ = writeln!(out, "    - `{}`", sl.text);
                    } else {
                        let _ = writeln!(out, "    - {}", sl.text);
                    }
                }
            }
            if let Some(footer) = &card.footer {
                let _ = writeln!(out, "  - _说明：{footer}_");
            }
        }
        out.push('\n');
    }

    // Callouts (payload schema / gaps / summary).
    for callout in &diag.callouts {
        let _ = writeln!(out, "### {}\n", callout.title);
        match callout.style {
            CalloutStyle::CodeBox => {
                out.push_str("```text\n");
                for line in &callout.lines {
                    let _ = writeln!(out, "{}", line.text);
                }
                out.push_str("```\n\n");
            }
            CalloutStyle::Bullets => {
                for line in &callout.lines {
                    let _ = writeln!(out, "- {}", line.text);
                }
                out.push('\n');
            }
        }
    }

    // Warning banner.
    if let Some(warn) = &diag.warning {
        let _ = writeln!(out, "> [!WARNING]\n> **{}**\n> {}\n", warn.title, warn.body);
    }

    if !diag.footer.is_empty() {
        let _ = writeln!(out, "_{}_\n", diag.footer);
    }
}

fn render_flow_table_md(table: &FlowTableSpec, out: &mut String) {
    let _ = writeln!(out, "## {}\n", table.heading);
    if !table.note.is_empty() {
        let _ = writeln!(out, "{}\n", table.note);
    }

    for grp in &table.groups {
        let _ = writeln!(out, "### {} ({})\n", grp.name, grp.rows.len());
        out.push_str("| 入口 / 动作 | 触发方 | 版本 | ① 核心入口 | ② 下游调用 | 说明 |\n");
        out.push_str("| :--- | :--- | :--- | :--- | :--- | :--- |\n");
        for row in &grp.rows {
            let entry = row
                .entry
                .iter()
                .map(|e| format!("`{}`", escape_md_cell(e)))
                .collect::<Vec<_>>()
                .join("<br>");
            let downstream = row
                .downstream
                .iter()
                .map(|d| escape_md_cell(d))
                .collect::<Vec<_>>()
                .join("<br>");
            let _ = writeln!(
                out,
                "| **{}** | {} | {} | {} | {} | {} |",
                escape_md_cell(&row.action),
                escape_md_cell(&row.trigger),
                escape_md_cell(&row.version),
                entry,
                downstream,
                escape_md_cell(&row.note),
            );
        }
        out.push('\n');
    }

    if let Some(hint) = &table.hint {
        let _ = writeln!(out, "> [!NOTE]\n> {hint}\n");
    }
}

fn mermaid_id(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() || s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, 'n');
    }
    s
}

fn escape_mermaid(raw: &str) -> String {
    raw.replace('"', "'").replace('\n', " ")
}

fn escape_md_cell(raw: &str) -> String {
    raw.replace('|', "\\|").replace('\n', " ")
}
